//! Explicit, disposable pinned-Hunk oracle execution, never a product dependency.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const PINS: [(&str, &str); 2] = [
    ("main", "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"),
    ("stable", "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"),
];

// This small adapter is generated only in a temporary directory. It imports the
// real pinned consumers; it is not a source mirror or a Rust parity substitute.
const DRIVER: &str = r#"
import { pathToFileURL } from 'node:url';
const root = process.argv[2];
const pin = process.argv[3];
const read = (path) => import(pathToFileURL(`${root}/test/review-conformance/${path}.ts`).href);
const consumers = await read('consumers');
const groups = [
  ['geometry', 'geometryFixtures', 'REVIEW_GEOMETRY_FIXTURES', 'REVIEW_GEOMETRY_CONSUMERS', 'project'],
  ['navigation', 'navigationFixtures', 'REVIEW_NAVIGATION_FIXTURES', 'REVIEW_NAVIGATION_CONSUMERS', 'project'],
  ['snapshot', 'snapshotFixtures', 'REVIEW_SNAPSHOT_FIXTURES', 'REVIEW_SNAPSHOT_CONSUMERS', 'project'],
  ['events', 'eventFixtures', 'REVIEW_EVENT_FIXTURES', 'REVIEW_EVENT_CONSUMERS', 'frame'],
];
const results = [];
for (const [group, module, fixtureKey, consumerKey, method] of groups) {
  const fixtures = (await read(module))[fixtureKey];
  for (const fixture of fixtures) {
    const actual = [];
    for (const consumer of consumers[consumerKey]) {
      const output = await consumer[method](fixture);
      if (!Bun.deepEquals(output, fixture.expected)) throw new Error(`${group}/${fixture.id}/${consumer.name} differs`);
      actual.push({ consumer: consumer.name, output });
    }
    results.push({ group, id: fixture.id, findings: fixture.findings, expected: fixture.expected, actual });
  }
}
const ordering = await read('orderingFixtures');
for (const fixture of ordering.REVIEW_PUBLICATION_ORDER_FIXTURES) {
  const actual = consumers.REVIEW_ORDERING_CONSUMERS.map(consumer => ({
    consumer: consumer.name, output: consumer.classify(fixture.current, fixture.incoming),
  }));
  for (const consumer of actual) {
    if (consumer.output !== fixture.expected) throw new Error(`ordering/${fixture.id}/${consumer.consumer} differs`);
  }
  results.push({ group: 'ordering', id: fixture.id, findings: fixture.findings,
    input: { current: fixture.current, incoming: fixture.incoming }, expected: fixture.expected, actual });
}
const source = path => import(pathToFileURL(`${root}/${path}.ts`).href);
const { ReviewProducer } = await source('src/app/review/producer');
const { createReviewStore } = await source('src/core/review/store');
const { classifyReviewPublication } = await source('src/core/review/generationOrder');
const { createTestDiffFile } = await source('test/helpers/diff-helpers');
for (const fixture of ordering.REVIEW_PRODUCER_ORDER_FIXTURES) {
  const files = [createTestDiffFile({ before: 'alpha\n', after: 'beta\n' })];
  const producer = new ReviewProducer({ files, sourceLabel: '/repo' }, { producerId: 'conformance' });
  producer.attachStore(createReviewStore(producer.getPublication().document));
  let previous = producer.getPublicationAddress();
  const output = fixture.steps.map((step, index) => {
    if (step.kind === 'reload') {
      producer.publish({ files, sourceLabel: '/repo' });
      producer.attachStore(createReviewStore(producer.getPublication().document));
    } else {
      producer.applyIntent({ type: 'filter/set', filter: `step-${index}` });
    }
    const next = producer.getPublicationAddress();
    const verdict = classifyReviewPublication(previous, next);
    previous = next;
    return verdict;
  });
  const expected = fixture.steps.map(step => step.expected);
  if (!Bun.deepEquals(output, expected)) throw new Error(`producer-ordering/${fixture.id} differs`);
  results.push({ group: 'producer-ordering', id: fixture.id, findings: fixture.findings,
    input: { steps: fixture.steps.map(step => step.kind) }, expected,
    actual: [{ consumer: 'producer ordering', output }] });
}
for (const fixture of (await read('wireFixtures')).REVIEW_WIRE_FIXTURES) {
  const actual = consumers.REVIEW_WIRE_CONSUMERS.map(consumer => ({
    consumer: consumer.name, output: consumer.parseAction(fixture.action),
  }));
  for (const consumer of actual) {
    if (!Bun.deepEquals(consumer.output, fixture.expected)) throw new Error(`wire/${fixture.id} differs`);
  }
  results.push({ group: 'wire', id: fixture.id, findings: fixture.findings,
    input: { action: fixture.action }, expected: fixture.expected, actual });
}
const { isBlankReviewNoteBody, planReviewIntent } = await source('src/core/review/intents');
const { createInitialReviewState } = await source('src/core/review/state');
const { createTestReviewDocument } = await source('test/helpers/review-store-helpers');
for (const fixture of (await read('noteBodies')).REVIEW_NOTE_BODY_FIXTURES) {
  const document = createTestReviewDocument(['alpha']);
  const state = { ...createInitialReviewState(document), draftNote: {
    id: 'draft:1', fileKey: document.files[0].key, hunkIndex: 0,
    side: 'new', line: 1, body: fixture.body,
  }};
  const plan = planReviewIntent(state, { type: 'notes/create-user', consumeDraft: true }, {
    noteId: 'user:1', timestamp: '2024-01-01T00:00:00.000Z',
  });
  const output = { blank: isBlankReviewNoteBody(fixture.body), actions: plan.actions.map(action => action.type) };
  const expected = { blank: fixture.blank, actions: [fixture.blank ? 'draft/cancel' : 'draft/save'] };
  if (!Bun.deepEquals(output, expected)) throw new Error(`note-body/${fixture.id} differs`);
  results.push({ group: 'note-body', id: fixture.id, input: { body: fixture.body }, expected,
    actual: [{ consumer: 'core note policy and draft planner', output }] });
}
const { MAX_REVIEW_NOTE_BYTES, reviewNoteWithinSizeLimit } = await source('src/core/review/noteSize');
for (const fixture of (await read('noteSize')).REVIEW_NOTE_SIZE_FIXTURES) {
  const note = fixture.build();
  const actual = [{ consumer: 'core note size', output: reviewNoteWithinSizeLimit(note) },
    ...consumers.REVIEW_WIRE_CONSUMERS.map(consumer => ({
      consumer: 'review wire note size', output: consumer.acceptsNote(note),
    }))];
  for (const consumer of actual) {
    if (consumer.output !== fixture.withinSizeLimit) throw new Error(`note-size/${fixture.id} differs`);
  }
  results.push({ group: 'note-size', id: fixture.id,
    input: { maxReviewNoteBytes: MAX_REVIEW_NOTE_BYTES,
      serializedBytes: new TextEncoder().encode(JSON.stringify(note)).byteLength },
    expected: fixture.withinSizeLimit, actual });
}
console.log(JSON.stringify({ schemaVersion: 1, upstream: pin, runtime: `bun ${Bun.version}`, results }, null, 2));
"#;

fn checked(command: &mut Command, label: &str) -> Result<Output> {
    let output = command.output().with_context(|| format!("run {label}"))?;
    if !output.status.success() {
        bail!(
            "{label} failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

pub fn capture(repo: &Path, bun: &Path) -> Result<()> {
    let bun = bun
        .canonicalize()
        .context("resolve explicit oracle runtime")?;
    let version = checked(
        Command::new(&bun).arg("--version"),
        "oracle runtime version",
    )?;
    if version.stdout != b"1.3.14\n" {
        bail!("the conformance oracle requires Bun 1.3.14");
    }
    let scratch = tempfile::tempdir().context("create disposable conformance checkout")?;
    let driver = scratch.path().join("capture.ts");
    fs::write(&driver, DRIVER)?;
    let mut captures = Vec::new();
    for (name, pin) in PINS {
        eprintln!("Capturing {name} review conformance at {pin}");
        let checkout = scratch.path().join(name);
        fs::create_dir(&checkout)?;
        let archive = checked(
            Command::new("git").current_dir(repo).args(["archive", pin]),
            "archive pinned Hunk",
        )?;
        tar::Archive::new(archive.stdout.as_slice()).unpack(&checkout)?;
        let config = scratch.path().join(format!("config-{name}"));
        fs::create_dir(&config)?;
        let command = || {
            let mut command = Command::new(&bun);
            command
                .current_dir(&checkout)
                .env("XDG_CONFIG_HOME", &config)
                .env("HUNK_MCP_DISABLE", "1")
                .env("HUNK_DISABLE_UPDATE_NOTICE", "1");
            command
        };
        checked(
            command().args(["install", "--frozen-lockfile", "--ignore-scripts"]),
            "install pinned oracle dependencies without lifecycle hooks",
        )?;
        let tests = checked(
            command().args(["test", "test/review-conformance/conformance.test.ts"]),
            "pinned conformance suite",
        )?;
        eprint!("{}", String::from_utf8_lossy(&tests.stderr));
        let output = checked(
            command().arg(&driver).arg(&checkout).arg(pin),
            "capture real consumer projections",
        )?;
        validate_capture(&output.stdout, pin)?;
        captures.push((name, output.stdout));
    }
    // Neither tracked fixture changes unless both source suites and captures pass.
    for (name, bytes) in captures {
        fs::write(
            repo.join(format!("port/hunk/oracles/review-conformance-{name}.json")),
            bytes,
        )?;
    }
    Ok(())
}

fn validate_capture(bytes: &[u8], pin: &str) -> Result<()> {
    let value: Value = serde_json::from_slice(bytes).context("decode conformance capture")?;
    if value["schemaVersion"] != 1 || value["upstream"] != pin || value["runtime"] != "bun 1.3.14" {
        bail!("invalid conformance capture identity");
    }
    let results = value["results"]
        .as_array()
        .context("missing conformance results")?;
    for group in [
        "geometry",
        "navigation",
        "snapshot",
        "events",
        "ordering",
        "producer-ordering",
        "wire",
        "note-body",
        "note-size",
    ] {
        if !results.iter().any(|case| case["group"] == group) {
            bail!("missing conformance group {group}");
        }
    }
    for case in results {
        let actual = case["actual"]
            .as_array()
            .context("missing actual consumers")?;
        if actual.is_empty() || case.get("expected").is_none() {
            bail!("empty consumer output or missing expected projection");
        }
        for consumer in actual {
            if consumer["output"] != case["expected"] {
                bail!("captured consumer diverged for {}", case["id"]);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_both_frozen_captures_and_rejects_tampering() {
        for (encoded, (_, pin)) in [
            include_bytes!("../../port/hunk/oracles/review-conformance-main.json").as_slice(),
            include_bytes!("../../port/hunk/oracles/review-conformance-stable.json").as_slice(),
        ]
        .into_iter()
        .zip(PINS)
        {
            validate_capture(encoded, pin).unwrap();
            let mut value: Value = serde_json::from_slice(encoded).unwrap();
            value["results"][0]["actual"][0]["output"] = Value::Null;
            assert!(validate_capture(&serde_json::to_vec(&value).unwrap(), pin).is_err());
            assert!(validate_capture(encoded, "wrong-pin").is_err());
        }
    }
}
