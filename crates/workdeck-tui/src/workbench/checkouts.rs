//! Retained native checkout contexts. Admission never widens a file provider.
use super::{WorkbenchOptions, WorkbenchShell, WorkbenchTab, host::NativeReturnContext};
use crate::{DynamicReviewHostOptions, ReviewApp};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use workdeck_core::{CliInput, VcsDiffCommandInput};
use workdeck_pm::{
    SourceSelector,
    registry::{RegisteredCheckout, RegistryNavigation},
};

const MAX_CONTEXTS: usize = 8;

#[derive(Debug)]
pub(crate) struct RetainedCheckout {
    pub shell: Mutex<WorkbenchShell>,
    binding: RegisteredCheckout,
    navigation: Option<RegistryNavigation>,
    review: NativeReturnContext,
    command_cwd: std::path::PathBuf,
    provider: Option<Arc<dyn super::RepositoryPanelProvider>>,
}
#[derive(Debug, Default)]
pub(crate) struct CheckoutContexts {
    pub retained: Vec<RetainedCheckout>,
    launch: Option<RegisteredCheckout>,
    navigation: Option<RegistryNavigation>,
    pending: Option<PendingCheckout>,
}
#[derive(Debug)]
struct PendingCheckout {
    binding: RegisteredCheckout,
    navigation: Option<RegistryNavigation>,
    provider_identity: Option<String>,
    committed: bool,
}
fn same_source(left: &RegisteredCheckout, right: &RegisteredCheckout) -> bool {
    left.checkout == right.checkout
        && left.repository == right.repository
        && left.checkout_binding == right.checkout_binding
        && left.source == right.source
}

fn source_label(navigation: &Option<RegistryNavigation>) -> String {
    navigation
        .as_ref()
        .map(|navigation| {
            let checkout = navigation.checkout();
            if checkout.source == SourceSelector::WorkingTree {
                checkout.alias.clone()
            } else {
                format!("{} · {:?} read-only", checkout.alias, checkout.source)
            }
        })
        .unwrap_or_else(|| "launch".into())
}

impl ReviewApp {
    pub(crate) fn prepare_workbench_checkout_host(
        &self,
        host: &mut DynamicReviewHostOptions,
    ) -> Result<(), String> {
        let binding = self.workbench.as_ref().and_then(|shell| {
            let shell = shell.lock().unwrap_or_else(|error| error.into_inner());
            shell.readonly.as_ref().and(shell.checkout_binding.clone())
        });
        let Some(binding) = binding else {
            return Ok(());
        };
        binding.resolve().map_err(|error| error.message)?;
        if host.repo_root.as_ref() != Some(&binding.checkout) {
            return Err("Read-only planning reload does not match its selected checkout".into());
        }
        host.registry_navigation = if let Some(pending) = &self.workbench_checkouts.pending {
            let navigation = pending
                .navigation
                .clone()
                .ok_or("Ref-backed switching requires explicit registry admission")?;
            navigation.revalidate().map_err(|error| error.message)?;
            Some(navigation)
        } else {
            None
        };
        // Code review still reads the selected physical checkout. Planning data
        // comes exclusively from the independently bound immutable ref readers.
        host.repository_panels = Some(None);
        Ok(())
    }

    pub(crate) fn pending_workbench_checkout(&self) -> Option<&RegisteredCheckout> {
        self.workbench_checkouts
            .pending
            .as_ref()
            .map(|pending| &pending.binding)
    }

    /// Called before any extension preparation or broker publication. Registered
    /// targets retain registry admission; returning home retains its physical ID.
    pub(crate) fn validate_workbench_checkout(
        &self,
        host: Option<&DynamicReviewHostOptions>,
    ) -> Result<(), String> {
        let Some(pending) = &self.workbench_checkouts.pending else {
            return Ok(());
        };
        pending.binding.resolve().map_err(|error| error.message)?;
        if let Some(navigation) = &pending.navigation {
            navigation.revalidate().map_err(|error| error.message)?;
        }
        let host = host.ok_or("Checkout switch requires resolved host options")?;
        if host.repo_root.as_ref() != Some(&pending.binding.checkout) {
            return Err("Checkout switch loaded a different repository".into());
        }
        let provider = host
            .repository_panels
            .as_ref()
            .and_then(|provider| provider.as_ref());
        if pending.binding.source == SourceSelector::WorkingTree {
            let source = provider
                .ok_or("Checkout switch requires a source-bound panel provider")?
                .source();
            if source.root != pending.binding.checkout {
                return Err("Checkout switch loaded a different repository".into());
            }
            if pending
                .provider_identity
                .as_ref()
                .is_some_and(|identity| identity != &source.identity)
            {
                return Err(
                    "Retained checkout panel source changed; its drafts remain retained".into(),
                );
            }
        } else if !matches!(host.repository_panels, Some(None)) {
            return Err("Ref-backed planning must not adopt native repository panels".into());
        }
        let cwd = host
            .command_cwd
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !cwd.starts_with(&pending.binding.checkout) {
            return Err("Checkout switch command cwd is outside its source".into());
        }
        if pending.navigation.is_some()
            && !host
                .registry_navigation
                .as_ref()
                .is_some_and(|navigation| same_source(navigation.checkout(), &pending.binding))
        {
            return Err("Checkout switch did not load its registered source".into());
        }
        Ok(())
    }

    pub(crate) fn mark_workbench_checkout_committed(&mut self) {
        if let Some(pending) = &mut self.workbench_checkouts.pending {
            pending.committed = true;
        }
    }

    pub(super) fn workbench_launch_checkout<R>(&mut self, reload: &mut R) -> Result<(), String>
    where
        R: FnMut(&mut ReviewApp, &CliInput, &Path) -> Result<(), String>,
    {
        let Some(launch) = self.workbench_checkouts.launch.clone() else {
            return Ok(());
        };
        self.switch_workbench_checkout(launch, None, reload)
    }

    pub(super) fn workbench_previous_checkout<R>(&mut self, reload: &mut R) -> Result<(), String>
    where
        R: FnMut(&mut ReviewApp, &CliInput, &Path) -> Result<(), String>,
    {
        let Some(previous) = self.workbench_checkouts.retained.last() else {
            return Err("No previous checkout in this Workdeck session".into());
        };
        self.switch_workbench_checkout(
            previous.binding.clone(),
            previous.navigation.clone(),
            reload,
        )
    }

    pub(super) fn workbench_registered_checkout<R>(
        &mut self,
        navigation: RegistryNavigation,
        reload: &mut R,
    ) -> Result<(), String>
    where
        R: FnMut(&mut ReviewApp, &CliInput, &Path) -> Result<(), String>,
    {
        navigation.revalidate().map_err(|error| error.message)?;
        self.switch_workbench_checkout(navigation.checkout().clone(), Some(navigation), reload)
    }

    fn switch_workbench_checkout<R>(
        &mut self,
        binding: RegisteredCheckout,
        navigation: Option<RegistryNavigation>,
        reload: &mut R,
    ) -> Result<(), String>
    where
        R: FnMut(&mut ReviewApp, &CliInput, &Path) -> Result<(), String>,
    {
        binding.resolve().map_err(|error| error.message)?;
        if let Some(navigation) = &navigation {
            navigation.revalidate().map_err(|error| error.message)?;
        }
        let current = self
            .workbench
            .as_ref()
            .ok_or("No mounted planning source")?;
        let (original_binding, planning, author, signal) = {
            let shell = current.lock().unwrap_or_else(|error| error.into_inner());
            (
                shell
                    .checkout_binding
                    .clone()
                    .ok_or("Current checkout has no retained native identity")?,
                shell.controller.return_context(),
                shell.options.author.clone(),
                shell.context.checks.signal.clone(),
            )
        };
        original_binding.resolve().map_err(|error| error.message)?;
        if same_source(&original_binding, &binding) {
            self.workbench_checkouts.navigation = navigation;
            current
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .checkout_label = Some(source_label(&self.workbench_checkouts.navigation));
            return Ok(());
        }
        if self.options.repo.as_ref() != Some(&original_binding.checkout) {
            return Err("Current review and planning roots differ; return the review to its planning checkout before switching".into());
        }
        let position = self
            .workbench_checkouts
            .retained
            .iter()
            .position(|entry| same_source(&entry.binding, &binding));
        if position.is_none() && self.workbench_checkouts.retained.len() + 1 >= MAX_CONTEXTS {
            return Err(format!(
                "This session retains {MAX_CONTEXTS} checkout contexts; existing drafts are preserved. Open another Workdeck session for additional checkouts"
            ));
        }
        let review = self.capture_native_return(planning);
        if review.input.is_none() {
            return Err("Current checkout has no reloadable review input".into());
        }
        let original_provider = self.options.repository_panels.clone();
        let original_cwd = self
            .options
            .command_cwd
            .clone()
            .unwrap_or_else(|| original_binding.checkout.clone());
        let mut target = if let Some(position) = position {
            self.workbench_checkouts.retained.remove(position)
        } else {
            let options = WorkbenchOptions {
                root: binding.checkout.clone(),
                author,
            };
            let mut shell = if binding.source == SourceSelector::WorkingTree {
                WorkbenchShell::open(options, true)
            } else {
                WorkbenchShell::open_readonly(options, binding.clone())?
            };
            if !shell
                .checkout_binding
                .as_ref()
                .is_some_and(|current| same_source(current, &binding))
            {
                return Err(
                    "Selected checkout changed while preparing its planning controller".into(),
                );
            }
            shell.attach_run_signal(Some(signal));
            let input = CliInput::Vcs(VcsDiffCommandInput {
                range: None,
                range_endpoints: None,
                staged: false,
                pathspecs: Vec::new(),
                options: self
                    .options
                    .review_input
                    .as_ref()
                    .map(|input| input.options().clone())
                    .unwrap_or_default(),
            });
            let mut initial = review.clone();
            initial.input = Some(input);
            initial.path = None;
            initial.filter.clear();
            initial.scroll = 0;
            initial.current_line_row = 0;
            initial.sources.clear();
            initial.expanded_gaps.clear();
            initial.gap_cursor_restore.clear();
            RetainedCheckout {
                shell: Mutex::new(shell),
                binding: binding.clone(),
                navigation: navigation.clone(),
                review: initial,
                command_cwd: binding.checkout.clone(),
                provider: None,
            }
        };
        let input = target
            .review
            .input
            .clone()
            .ok_or("Retained checkout has no reloadable review input")?;
        let target_review = target.review.clone();
        let pending = PendingCheckout {
            binding: binding.clone(),
            navigation: navigation.clone(),
            provider_identity: target
                .provider
                .as_ref()
                .map(|provider| provider.source().identity),
            committed: false,
        };
        let original = self.workbench.take().expect("checked mounted shell");
        // The registry browser belongs to the launch source and follows the user.
        // Per-checkout native forms and workers stay in their original shell.
        let my_work = original
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .my_work
            .take();
        target
            .shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .my_work = my_work;
        self.workbench = Some(target.shell);
        self.workbench_checkouts.pending = Some(pending);
        let result = reload(self, &input, &target.command_cwd);
        let pending = self
            .workbench_checkouts
            .pending
            .take()
            .expect("prepared transition");
        if !pending.committed {
            target.shell = self.workbench.take().expect("staged target");
            let my_work = target
                .shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .my_work
                .take();
            original
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .my_work = my_work;
            self.workbench = Some(original);
            if let Some(position) = position {
                self.workbench_checkouts.retained.insert(position, target);
            }
            return result.and(Err(
                "Checkout loader did not commit its prepared source".into()
            ));
        }
        self.workbench_checkouts
            .launch
            .get_or_insert_with(|| original_binding.clone());
        let previous_navigation = self.workbench_checkouts.navigation.take();
        original
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .checkout_label = Some(source_label(&previous_navigation));
        self.workbench_checkouts.retained.push(RetainedCheckout {
            shell: original,
            binding: original_binding,
            navigation: previous_navigation,
            review,
            command_cwd: original_cwd,
            provider: original_provider,
        });
        self.workbench_checkouts.navigation = navigation;
        if let Some(shell) = &self.workbench {
            let mut shell = shell.lock().unwrap_or_else(|error| error.into_inner());
            if shell.panels.is_none() {
                shell.attach_panels(self.options.repository_panels.clone());
            }
            shell.checkout_label = Some(source_label(&self.workbench_checkouts.navigation));
            shell.notice = Some(format!("Checkout: {}", binding.checkout.display()));
            if shell.tab == WorkbenchTab::MyWork {
                shell.tab = shell.my_work_return_tab;
            }
        }
        self.options.workbench = self.workbench.as_ref().map(|shell| {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .options
                .clone()
        });
        self.restore_native_review(&target_review);
        result
    }
}

#[cfg(test)]
#[path = "checkout_tests.rs"]
mod tests;
