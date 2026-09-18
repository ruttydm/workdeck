//! Install-aware startup update notices and persisted version-change state.

use std::path::PathBuf;

use serde_json::{Map, Value};
use workdeck_core::{StartupNotice, WorkdeckInstallSource, resolve_app_state_path};
use workdeck_store::{read_app_state_record, update_app_state_record};

use crate::update::{
    ChannelVersions, InstallSourceFacts, SelfUpdateContext, detect_install_source,
    fetch_channel_versions,
};
use crate::version::{
    UNKNOWN_CLI_VERSION, is_comparable_version, is_newer_version, is_stable_version,
};

pub const DISABLE_STARTUP_UPDATE_NOTICE_ENV: &str = "WORKDECK_DISABLE_UPDATE_NOTICE";
const STARTUP_STATE_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateChannel {
    Latest,
    Beta,
}

impl UpdateChannel {
    const fn name(self) -> &'static str {
        match self {
            Self::Latest => "latest",
            Self::Beta => "beta",
        }
    }
}

/// Dependencies for one non-blocking startup update lookup.
#[derive(Clone)]
pub struct StartupUpdateNoticeContext {
    pub update: SelfUpdateContext,
    pub state_path: Option<PathBuf>,
}

impl StartupUpdateNoticeContext {
    pub fn current() -> Result<Self, crate::update::UpdateError> {
        Ok(Self {
            update: SelfUpdateContext::current()?,
            state_path: resolve_app_state_path(),
        })
    }
}

fn suppresses_notices(source: WorkdeckInstallSource) -> bool {
    source == WorkdeckInstallSource::Dev
}

fn update_instruction(
    channel: UpdateChannel,
    version: &str,
    source: WorkdeckInstallSource,
) -> String {
    if source == WorkdeckInstallSource::Nix {
        return "update Workdeck through your Nix configuration".into();
    }
    if channel == UpdateChannel::Latest {
        "run `workdeck update`".into()
    } else {
        format!("run `workdeck update {version}`")
    }
}

fn create_update_notice(
    version: &str,
    channel: UpdateChannel,
    source: WorkdeckInstallSource,
) -> StartupNotice {
    StartupNotice::new(
        format!("{}:{version}", channel.name()),
        format!(
            "Update available: {version} ({}) • {}",
            channel.name(),
            update_instruction(channel, version, source)
        ),
    )
}

fn select_update_notice(
    installed_version: &str,
    versions: &ChannelVersions,
    source: WorkdeckInstallSource,
) -> Option<StartupNotice> {
    if !is_comparable_version(installed_version) {
        return None;
    }
    let latest = versions.latest.as_deref();
    let beta = (source == WorkdeckInstallSource::Cargo)
        .then_some(versions.beta.as_deref())
        .flatten();

    if is_stable_version(installed_version) {
        if let Some(latest) = latest.filter(|version| is_newer_version(installed_version, version))
        {
            return Some(create_update_notice(latest, UpdateChannel::Latest, source));
        }
        return beta
            .filter(|version| is_newer_version(installed_version, version))
            .map(|version| create_update_notice(version, UpdateChannel::Beta, source));
    }

    let latest = latest.filter(|version| is_newer_version(installed_version, version));
    let beta = beta.filter(|version| is_newer_version(installed_version, version));
    match (latest, beta) {
        (Some(latest), Some(beta)) if is_newer_version(latest, beta) => {
            Some(create_update_notice(beta, UpdateChannel::Beta, source))
        }
        (Some(latest), _) => Some(create_update_notice(latest, UpdateChannel::Latest, source)),
        (None, Some(beta)) => Some(create_update_notice(beta, UpdateChannel::Beta, source)),
        (None, None) => None,
    }
}

fn resolve_skill_refresh_notice(context: &StartupUpdateNoticeContext) -> Option<StartupNotice> {
    let installed_version = &context.update.installed_version;
    if installed_version == UNKNOWN_CLI_VERSION {
        return None;
    }
    let path = context.state_path.as_ref()?;
    let record = read_app_state_record(path);
    let previous = record
        .get("lastSeenCliVersion")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut patch = Map::new();
    patch.insert("version".into(), Value::from(STARTUP_STATE_VERSION));
    patch.insert(
        "lastSeenCliVersion".into(),
        Value::String(installed_version.clone()),
    );
    update_app_state_record(path, patch).ok()?;
    previous
        .filter(|previous| previous != installed_version)
        .map(|_| {
            StartupNotice::new(
                format!("skill:{installed_version}"),
                format!(
                    "Workdeck {installed_version} installed • If your agent copied Workdeck's skill, run workdeck skill path"
                ),
            )
        })
}

/// Resolve the transient startup notice from local state and the install source's registry.
#[must_use]
pub fn resolve_startup_update_notice(
    context: &StartupUpdateNoticeContext,
) -> Option<StartupNotice> {
    if context
        .update
        .env
        .get(DISABLE_STARTUP_UPDATE_NOTICE_ENV)
        .is_some_and(|value| value == "1")
    {
        return None;
    }
    if let Some(notice) = resolve_skill_refresh_notice(context) {
        return Some(notice);
    }

    let detected = detect_install_source(&InstallSourceFacts {
        env: &context.update.env,
        executable_path: &context.update.executable_path,
        version: &context.update.installed_version,
        home_dir: context
            .update
            .env
            .get("HOME")
            .or_else(|| context.update.env.get("USERPROFILE"))
            .map(String::as_str),
        platform: context.update.platform,
        realpath: None,
    });
    let source = context.update.install_source.unwrap_or(detected);
    if suppresses_notices(source) {
        return None;
    }

    // Nix owns replacement but follows Workdeck's stable GitHub release stream.
    let lookup_source = if source == WorkdeckInstallSource::Nix {
        WorkdeckInstallSource::Direct
    } else {
        source
    };
    let versions = fetch_channel_versions(lookup_source, &context.update.release_lookup);
    select_update_notice(&context.update.installed_version, &versions, source)
}

#[cfg(test)]
mod tests;
