/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::entry::{ChangeSet, ChangelogEntries, ChangelogEntry};
use anyhow::{bail, Context, Result};
use clap::Parser;
use ordinal::Ordinal;
use serde::Serialize;
use smithy_rs_tool_common::changelog::{
    Changelog, HandAuthoredEntry, Reference, SdkModelChangeKind, SdkModelEntry,
};
use smithy_rs_tool_common::git::{find_git_repository_root, Git, GitCLI};
use std::env;
use std::fmt::Write;
use std::path::PathBuf;
use time::OffsetDateTime;

pub const EXAMPLE_ENTRY: &str = r#"
# Example changelog entries
# [[aws-sdk-rust]]
# message = "Fix typos in module documentation for generated crates"
# references = ["smithy-rs#920"]
# meta = { "breaking" = false, "tada" = false, "bug" = false }
# author = "rcoh"
#
# [[smithy-rs]]
# message = "Fix typos in module documentation for generated crates"
# references = ["smithy-rs#920"]
# meta = { "breaking" = false, "tada" = false, "bug" = false, "target" = "client | server | all"}
# author = "rcoh"
"#;

pub const USE_UPDATE_CHANGELOGS: &str =
    "<!-- Do not manually edit this file. Use the `changelogger` tool. -->";

fn maintainers() -> Vec<&'static str> {
    include_str!("../smithy-rs-maintainers.txt")
        .lines()
        .collect()
}

#[derive(Parser, Debug, Eq, PartialEq)]
pub struct RenderArgs {
    /// Which set of changes to render
    #[clap(long, action)]
    pub change_set: ChangeSet,
    /// Whether or not independent crate versions are being used (defaults to false)
    #[clap(long, action)]
    pub independent_versioning: bool,
    /// Source changelog entries to render
    #[clap(long, action, required(true))]
    pub source: Vec<PathBuf>,
    /// Which source to overwrite with an empty changelog template
    #[clap(long, action)]
    pub source_to_truncate: PathBuf,
    #[clap(long, action)]
    pub changelog_output: PathBuf,
    /// Optional path to output a release manifest file to
    #[clap(long, action)]
    pub release_manifest_output: Option<PathBuf>,
    /// Optional path to the SDK's versions.toml file for the previous release.
    /// This is used to filter out changelog entries that have `since_commit` information.
    #[clap(long, action)]
    pub previous_release_versions_manifest: Option<PathBuf>,
    // Location of the smithy-rs repository. If not specified, the current
    // working directory will be used to attempt to find it.
    #[clap(long, action)]
    pub smithy_rs_location: Option<PathBuf>,

    // For testing only
    #[clap(skip)]
    pub date_override: Option<OffsetDateTime>,
}

pub fn subcommand_render(args: &RenderArgs) -> Result<()> {
    let now = args.date_override.unwrap_or_else(OffsetDateTime::now_utc);

    let current_dir = env::current_dir()?;
    let repo_root: PathBuf = find_git_repository_root(
        "smithy-rs",
        args.smithy_rs_location
            .as_deref()
            .unwrap_or_else(|| current_dir.as_path()),
    )
    .context("failed to find smithy-rs repo root")?;
    let smithy_rs = GitCLI::new(&repo_root)?;

    if args.independent_versioning {
        let smithy_rs_metadata =
            date_based_release_metadata(now, "smithy-rs-release-manifest.json");
        let sdk_metadata = date_based_release_metadata(now, "aws-sdk-rust-release-manifest.json");
        update_changelogs(args, &smithy_rs, &smithy_rs_metadata, &sdk_metadata)
    } else {
        bail!("the --independent-versioning flag must be set; synchronized versioning no longer supported");
    }
}

struct ReleaseMetadata {
    title: String,
    tag: String,
    manifest_name: String,
}

#[derive(Serialize)]
struct ReleaseManifest {
    #[serde(rename = "tagName")]
    tag_name: String,
    name: String,
    body: String,
    prerelease: bool,
}

fn date_based_release_metadata(
    now: OffsetDateTime,
    manifest_name: impl Into<String>,
) -> ReleaseMetadata {
    ReleaseMetadata {
        title: date_title(&now),
        tag: format!(
            "release-{year}-{month:02}-{day:02}",
            year = now.date().year(),
            month = u8::from(now.date().month()),
            day = now.date().day()
        ),
        manifest_name: manifest_name.into(),
    }
}

fn date_title(now: &OffsetDateTime) -> String {
    format!(
        "{month} {day}, {year}",
        month = now.date().month(),
        day = Ordinal(now.date().day()),
        year = now.date().year()
    )
}

fn render_model_entry(entry: &SdkModelEntry, out: &mut String) {
    write!(
        out,
        "- `{module}` ({version}): {message}",
        module = entry.module,
        version = entry.version,
        message = entry.message
    )
    .unwrap();
}

fn to_md_link(reference: &Reference) -> String {
    format!(
        "[{repo}#{number}](https://github.com/awslabs/{repo}/issues/{number})",
        repo = reference.repo,
        number = reference.number
    )
}

/// Write a changelog entry to [out]
///
/// Example output:
/// `- Add a feature (smithy-rs#123, @contributor)`
fn render_entry(entry: &HandAuthoredEntry, mut out: &mut String) {
    let mut meta = String::new();
    if entry.meta.bug {
        meta.push('🐛');
    }
    if entry.meta.breaking {
        meta.push('⚠');
    }
    if entry.meta.tada {
        meta.push('🎉');
    }
    if !meta.is_empty() {
        meta.push(' ');
    }
    let mut references = entry
        .meta
        .target
        .iter()
        .map(|t| t.to_string())
        .chain(entry.references.iter().map(to_md_link))
        .collect::<Vec<_>>();
    if !maintainers().contains(&entry.author.to_ascii_lowercase().as_str()) {
        references.push(format!("@{}", entry.author.to_ascii_lowercase()));
    };
    if !references.is_empty() {
        write!(meta, "({}) ", references.join(", ")).unwrap();
    }
    write!(
        &mut out,
        "- {meta}{message}",
        meta = meta,
        message = indented_message(&entry.message),
    )
    .unwrap();
}

fn indented_message(message: &str) -> String {
    let mut out = String::new();
    for (idx, line) in message.lines().enumerate() {
        if idx > 0 {
            out.push('\n');
            if !line.is_empty() {
                out.push_str("    ");
            }
        }
        out.push_str(line);
    }
    out
}

fn load_changelogs(args: &RenderArgs) -> Result<Changelog> {
    let mut combined = Changelog::new();
    for source in &args.source {
        let changelog = Changelog::load_from_file(source)
            .map_err(|errs| anyhow::Error::msg(format!("failed to load {source:?}: {errs:#?}")))?;
        changelog.validate().map_err(|errs| {
            anyhow::Error::msg(format!(
                "failed to load {source:?}: {errors}",
                errors = errs.join("\n")
            ))
        })?;
        combined.merge(changelog);
    }
    Ok(combined)
}

fn update_changelogs(
    args: &RenderArgs,
    smithy_rs: &dyn Git,
    smithy_rs_metadata: &ReleaseMetadata,
    aws_sdk_rust_metadata: &ReleaseMetadata,
) -> Result<()> {
    let changelog = load_changelogs(args)?;
    let release_metadata = match args.change_set {
        ChangeSet::AwsSdk => aws_sdk_rust_metadata,
        ChangeSet::SmithyRs => smithy_rs_metadata,
    };
    let entries = ChangelogEntries::from(changelog);
    let entries = entries.filter(
        smithy_rs,
        args.change_set,
        args.previous_release_versions_manifest.as_deref(),
    )?;
    let (release_header, release_notes) = render(&entries, &release_metadata.title);
    if let Some(output_path) = &args.release_manifest_output {
        let release_manifest = ReleaseManifest {
            tag_name: release_metadata.tag.clone(),
            name: release_metadata.title.clone(),
            body: release_notes.clone(),
            // All releases are pre-releases for now
            prerelease: true,
        };
        std::fs::write(
            output_path.join(&release_metadata.manifest_name),
            serde_json::to_string_pretty(&release_manifest)?,
        )
        .context("failed to write release manifest")?;
    }

    let mut update = USE_UPDATE_CHANGELOGS.to_string();
    update.push('\n');
    update.push_str(&release_header);
    update.push_str(&release_notes);

    let current = std::fs::read_to_string(&args.changelog_output)
        .context("failed to read rendered destination changelog")?
        .replace(USE_UPDATE_CHANGELOGS, "");
    update.push_str(&current);
    std::fs::write(&args.changelog_output, update).context("failed to write rendered changelog")?;

    std::fs::write(&args.source_to_truncate, EXAMPLE_ENTRY.trim())
        .context("failed to truncate source")?;
    eprintln!("Changelogs updated!");
    Ok(())
}

fn render_handauthored<'a>(entries: impl Iterator<Item = &'a HandAuthoredEntry>, out: &mut String) {
    let (breaking, non_breaking) = entries.partition::<Vec<_>, _>(|entry| entry.meta.breaking);

    if !breaking.is_empty() {
        out.push_str("**Breaking Changes:**\n");
        for change in breaking {
            render_entry(change, out);
            out.push('\n');
        }
        out.push('\n')
    }

    if !non_breaking.is_empty() {
        out.push_str("**New this release:**\n");
        for change in non_breaking {
            render_entry(change, out);
            out.push('\n');
        }
        out.push('\n');
    }
}

fn render_sdk_model_entries<'a>(
    entries: impl Iterator<Item = &'a SdkModelEntry>,
    out: &mut String,
) {
    let (features, docs) =
        entries.partition::<Vec<_>, _>(|entry| matches!(entry.kind, SdkModelChangeKind::Feature));
    if !features.is_empty() {
        out.push_str("**Service Features:**\n");
        for entry in features {
            render_model_entry(entry, out);
            out.push('\n');
        }
        out.push('\n');
    }
    if !docs.is_empty() {
        out.push_str("**Service Documentation:**\n");
        for entry in docs {
            render_model_entry(entry, out);
            out.push('\n');
        }
        out.push('\n');
    }
}

/// Convert a list of changelog entries into markdown.
/// Returns (header, body)
fn render(entries: &[ChangelogEntry], release_header: &str) -> (String, String) {
    let mut header = String::new();
    header.push_str(release_header);
    header.push('\n');
    for _ in 0..release_header.len() {
        header.push('=');
    }
    header.push('\n');

    let mut out = String::new();
    render_handauthored(
        entries.iter().filter_map(ChangelogEntry::hand_authored),
        &mut out,
    );
    render_sdk_model_entries(
        entries.iter().filter_map(ChangelogEntry::aws_sdk_model),
        &mut out,
    );

    let mut external_contribs = entries
        .iter()
        .filter_map(|entry| entry.hand_authored().map(|e| e.author.to_ascii_lowercase()))
        .filter(|author| !maintainers().contains(&author.as_str()))
        .collect::<Vec<_>>();
    external_contribs.sort();
    external_contribs.dedup();
    if !external_contribs.is_empty() {
        out.push_str("**Contributors**\nThank you for your contributions! ❤\n");
        for contributor_handle in external_contribs {
            // retrieve all contributions this author made
            let mut contribution_references = entries
                .iter()
                .filter(|entry| {
                    entry
                        .hand_authored()
                        .map(|e| e.author.eq_ignore_ascii_case(contributor_handle.as_str()))
                        .unwrap_or(false)
                })
                .flat_map(|entry| {
                    entry
                        .hand_authored()
                        .unwrap()
                        .references
                        .iter()
                        .map(to_md_link)
                })
                .collect::<Vec<_>>();
            contribution_references.sort();
            contribution_references.dedup();
            let contribution_references = contribution_references.as_slice().join(", ");
            out.push_str("- @");
            out.push_str(&contributor_handle);
            if !contribution_references.is_empty() {
                out.push_str(&format!(" ({})", contribution_references));
            }
            out.push('\n');
        }
    }

    (header, out)
}

#[cfg(test)]
mod test {
    use super::{date_based_release_metadata, render, Changelog, ChangelogEntries, ChangelogEntry};
    use smithy_rs_tool_common::changelog::SdkAffected;
    use time::OffsetDateTime;

    fn render_full(entries: &[ChangelogEntry], release_header: &str) -> String {
        let (header, body) = render(entries, release_header);
        return format!("{}{}", header, body);
    }

    #[test]
    fn end_to_end_changelog() {
        let changelog_toml = r#"
[[smithy-rs]]
author = "rcoh"
message = "I made a major change to update the code generator"
meta = { breaking = true, tada = false, bug = false }
references = ["smithy-rs#445"]

[[smithy-rs]]
author = "external-contrib"
message = "I made a change to update the code generator"
meta = { breaking = false, tada = true, bug = false }
references = ["smithy-rs#446"]

[[smithy-rs]]
author = "another-contrib"
message = "I made a minor change"
meta = { breaking = false, tada = false, bug = false }
references = ["smithy-rs#200"]

[[aws-sdk-rust]]
author = "rcoh"
message = "I made a major change to update the AWS SDK"
meta = { breaking = true, tada = false, bug = false }
references = ["smithy-rs#445"]

[[aws-sdk-rust]]
author = "external-contrib"
message = "I made a change to update the code generator"
meta = { breaking = false, tada = true, bug = false }
references = ["smithy-rs#446"]

[[smithy-rs]]
author = "external-contrib"
message = """
I made a change to update the code generator

**Update guide:**
blah blah
"""
meta = { breaking = false, tada = true, bug = false }
references = ["smithy-rs#446"]

[[aws-sdk-model]]
module = "aws-sdk-s3"
version = "0.14.0"
kind = "Feature"
message = "Some new API to do X"

[[aws-sdk-model]]
module = "aws-sdk-ec2"
version = "0.12.0"
kind = "Documentation"
message = "Updated some docs"

[[aws-sdk-model]]
module = "aws-sdk-ec2"
version = "0.12.0"
kind = "Feature"
message = "Some API change"
        "#;
        let changelog: Changelog = Changelog::parse_str(changelog_toml).expect("valid changelog");
        let ChangelogEntries {
            aws_sdk_rust,
            smithy_rs,
        } = changelog.into();

        let smithy_rs_rendered = render_full(&smithy_rs, "v0.3.0 (January 4th, 2022)");
        let smithy_rs_expected = r#"
v0.3.0 (January 4th, 2022)
==========================
**Breaking Changes:**
- ⚠ (all, [smithy-rs#445](https://github.com/awslabs/smithy-rs/issues/445)) I made a major change to update the code generator

**New this release:**
- 🎉 (all, [smithy-rs#446](https://github.com/awslabs/smithy-rs/issues/446), @external-contrib) I made a change to update the code generator
- 🎉 (all, [smithy-rs#446](https://github.com/awslabs/smithy-rs/issues/446), @external-contrib) I made a change to update the code generator

    **Update guide:**
    blah blah
- (all, [smithy-rs#200](https://github.com/awslabs/smithy-rs/issues/200), @another-contrib) I made a minor change

**Contributors**
Thank you for your contributions! ❤
- @another-contrib ([smithy-rs#200](https://github.com/awslabs/smithy-rs/issues/200))
- @external-contrib ([smithy-rs#446](https://github.com/awslabs/smithy-rs/issues/446))
"#
        .trim_start();
        pretty_assertions::assert_str_eq!(smithy_rs_expected, smithy_rs_rendered);

        let aws_sdk_rust_rendered = render_full(&aws_sdk_rust, "v0.1.0 (January 4th, 2022)");
        let aws_sdk_expected = r#"
v0.1.0 (January 4th, 2022)
==========================
**Breaking Changes:**
- ⚠ ([smithy-rs#445](https://github.com/awslabs/smithy-rs/issues/445)) I made a major change to update the AWS SDK

**New this release:**
- 🎉 ([smithy-rs#446](https://github.com/awslabs/smithy-rs/issues/446), @external-contrib) I made a change to update the code generator

**Service Features:**
- `aws-sdk-ec2` (0.12.0): Some API change
- `aws-sdk-s3` (0.14.0): Some new API to do X

**Service Documentation:**
- `aws-sdk-ec2` (0.12.0): Updated some docs

**Contributors**
Thank you for your contributions! ❤
- @external-contrib ([smithy-rs#446](https://github.com/awslabs/smithy-rs/issues/446))
"#
        .trim_start();
        pretty_assertions::assert_str_eq!(aws_sdk_expected, aws_sdk_rust_rendered);
    }

    #[test]
    fn test_date_based_release_metadata() {
        let now = OffsetDateTime::from_unix_timestamp(100_000_000).unwrap();
        let result = date_based_release_metadata(now, "some-manifest.json");
        assert_eq!("March 3rd, 1973", result.title);
        assert_eq!("release-1973-03-03", result.tag);
        assert_eq!("some-manifest.json", result.manifest_name);
    }

    #[test]
    fn test_partition_client_server() {
        let sample = r#"
[[smithy-rs]]
author = "external-contrib"
message = """
this is a multiline
message
"""
meta = { breaking = false, tada = true, bug = false, target = "server" }
references = ["smithy-rs#446"]

[[aws-sdk-model]]
module = "aws-sdk-s3"
version = "0.14.0"
kind = "Feature"
message = "Some new API to do X"

[[smithy-rs]]
author = "external-contrib"
message = "a client message"
meta = { breaking = false, tada = true, bug = false, target = "client" }
references = ["smithy-rs#446"]

[[smithy-rs]]
message = "a change for both"
meta = { breaking = false, tada = true, bug = false, target = "all" }
references = ["smithy-rs#446"]
author = "rcoh"

[[smithy-rs]]
message = "a missing sdk meta"
meta = { breaking = false, tada = true, bug = false }
references = ["smithy-rs#446"]
author = "rcoh"
"#;
        let changelog: Changelog = Changelog::parse_str(sample).expect("valid changelog");
        let ChangelogEntries {
            aws_sdk_rust: _,
            smithy_rs,
        } = changelog.into();
        let affected = vec![
            SdkAffected::Server,
            SdkAffected::Client,
            SdkAffected::All,
            SdkAffected::All,
        ];
        let entries = smithy_rs
            .iter()
            .filter_map(ChangelogEntry::hand_authored)
            .zip(affected)
            .collect::<Vec<_>>();
        for (e, a) in entries {
            assert_eq!(e.meta.target, Some(a));
        }
    }

    #[test]
    fn test_empty_render() {
        let smithy_rs = Vec::<ChangelogEntry>::new();
        let (release_title, release_notes) = render(&smithy_rs, "some header");

        assert_eq!(release_title, "some header\n===========\n");
        assert_eq!(release_notes, "");
    }
}
