# Counted alpha file navigation

`source_controller::tests::counted_alpha_file_navigation_commits_only_final_selection`
uses Hunk's alpha8-to-800 fixture followed by one-line beta, gamma and delta
changes. A single File-scope move of three selects delta hunk 0 and increments
native selection state revision exactly once. A repeated forward move at the
end leaves the revision and scroll unchanged.

The source case `moves across several files in one counted selection request`
passed on main `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` under disposable Bun 1.3.14:
one test and six assertions per pin. The focused native test passed in
0.87 seconds, with formatting and diff checks passing.

This is supplemental evidence, not a source-test mapping. One native state
revision is not proof of the source's exact file-top alignment request count.
The alignment emission and geometry obligation remains open. No runtime or
ledger disposition changed.

All 1,226 TUI library tests passed (9.15 seconds).
