+++
title = "Terminal and platform compatibility"
description = "Terminal capabilities behind mouse, color, copy and layout behavior."
template = "docs.html"
+++

Workdeck targets macOS, Linux and Windows. The complete native platform matrix is still a release gate, not a verified support claim. The Rust executable needs no Node.js runtime and is not distributed through npm. Git is recommended; Jujutsu and Sapling support requires their respective command-line tools.

## Terminal capabilities

Workdeck is a terminal-native application built on Ratatui and Crossterm. The best experience needs:

- a modern terminal with Unicode and truecolor support
- alternate-screen and mouse protocol support
- enough columns for split mode (auto mode stacks on narrow screens)
- OSC 52 support when Workdeck copies through the terminal clipboard protocol

If a terminal omits one capability, keyboard navigation and stack layout remain the safest fallback.

## Remote sessions and multiplexers

SSH, tmux, and similar layers can filter mouse, clipboard, keyboard, or color sequences. Verify the behavior in the underlying terminal, then ensure each intermediate layer forwards the relevant protocol. Use keyboard shortcuts when mouse reporting is captured by a multiplexer.

## Windows notes

Run Workdeck in a modern Windows terminal. Use the native executable; npm and mise installation instructions from upstream do not apply. Windows smoke and installation qualification remain pending. Repository paths and config locations follow platform conventions; examples use shell-neutral Workdeck arguments where possible. Git Bash, PowerShell, and other shells quote commands differently, so adapt multi-line shell examples to your environment.

## Light and dark backgrounds

`--theme auto` queries the terminal. If no answer arrives, Workdeck uses the dark default. Choose `github-light-default` or another explicit theme when terminal reporting, transparency, or remote layers make detection unreliable.

For a specific failure, continue with [Troubleshooting](/docs/help/troubleshooting/).

Adapted from Hunk's pinned MIT documentation, Copyright Modem Labs Inc.
Upstream OpenTUI and npm requirements were replaced with the native product
boundary; those historical dependencies are not Workdeck installation options.
This source interval remains unmapped pending complete migration verification.
