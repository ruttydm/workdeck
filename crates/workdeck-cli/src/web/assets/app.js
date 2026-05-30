(function () {
  var tabs = ["changes", "git", "files", "issues", "agents", "search"];
  var state = {
    snapshot: null,
    activeTab: "changes",
    selectedKey: null,
    collapsed: new Set(),
    searchQuery: "",
    searchResults: []
  };

  var listPane = document.getElementById("listPane");
  var previewPane = document.getElementById("previewPane");
  var tabsNode = document.getElementById("tabs");
  var statusNode = document.getElementById("status");

  function title(value) {
    return value.charAt(0).toUpperCase() + value.slice(1);
  }

  function text(value) {
    return value == null ? "" : String(value);
  }

  function keyFor(kind, id) {
    return kind + ":" + id;
  }

  function setStatus(value) {
    statusNode.textContent = value;
  }

  function render() {
    renderTabs();
    if (!state.snapshot) {
      listPane.innerHTML = '<div class="empty">Loading</div>';
      return;
    }
    if (state.activeTab === "changes") renderChanges();
    if (state.activeTab === "git") renderGit();
    if (state.activeTab === "files") renderFiles();
    if (state.activeTab === "issues") renderIssues();
    if (state.activeTab === "agents") renderAgents();
    if (state.activeTab === "search") renderSearch();
    setStatus("gen " + state.snapshot.generation + " - " + state.snapshot.repo_root);
  }

  function renderTabs() {
    tabsNode.innerHTML = "";
    tabs.forEach(function (tab) {
      var button = document.createElement("button");
      button.className = "tab" + (tab === state.activeTab ? " active" : "");
      button.textContent = title(tab);
      button.onclick = function () {
        state.activeTab = tab;
        state.selectedKey = null;
        previewPane.innerHTML = '<div class="empty">Select an item</div>';
        render();
      };
      tabsNode.appendChild(button);
    });
  }

  function section(label, rows) {
    if (!rows.length && label !== "Summary") return "";
    return '<div class="section"><div class="section-title">' + escapeHtml(label) + "</div>" + rows.join("") + "</div>";
  }

  function row(options) {
    var selected = options.key && options.key === state.selectedKey;
    return '<button class="row' + (selected ? " selected" : "") + '" data-action="' + escapeHtml(options.action || "") + '" data-key="' + escapeHtml(options.key || "") + '" data-id="' + escapeHtml(options.id || "") + '">' +
      '<span class="row-main">' + options.main + '</span>' +
      '<span class="row-detail">' + escapeHtml(options.detail || "") + '</span>' +
      '</button>';
  }

  function bindRows() {
    listPane.querySelectorAll("[data-action]").forEach(function (node) {
      node.onclick = function () {
        var action = node.getAttribute("data-action");
        var id = node.getAttribute("data-id");
        var key = node.getAttribute("data-key");
        if (action === "toggle") {
          if (state.collapsed.has(id)) state.collapsed.delete(id);
          else state.collapsed.add(id);
          render();
          return;
        }
        state.selectedKey = key;
        render();
        if (action) loadPreview(action, id);
      };
    });
  }

  function renderChanges() {
    var changes = (state.snapshot.status && state.snapshot.status.changes) || [];
    if (!changes.length) {
      listPane.innerHTML = '<div class="empty">Clean worktree</div>';
      return;
    }
    var tree = buildTree(changes, changePath);
    listPane.innerHTML = section("Changes", renderTree(tree, "change"));
    bindRows();
  }

  function renderFiles() {
    var files = state.snapshot.files || [];
    if (!files.length) {
      listPane.innerHTML = '<div class="empty">No tracked files</div>';
      return;
    }
    var tree = buildTree(files.map(function (path) { return { path: path }; }), function (item) { return item.path; });
    listPane.innerHTML = section("Files", renderTree(tree, "file"));
    bindRows();
  }

  function buildTree(items, getPath) {
    var root = { name: "", path: "", dirs: new Map(), files: [], totals: { files: 0, additions: 0, deletions: 0 } };
    items.forEach(function (item) {
      var parts = getPath(item).split("/").filter(Boolean);
      var node = root;
      parts.slice(0, -1).forEach(function (part, index) {
        var dirPath = parts.slice(0, index + 1).join("/");
        if (!node.dirs.has(part)) {
          node.dirs.set(part, { name: part, path: dirPath, dirs: new Map(), files: [], totals: { files: 0, additions: 0, deletions: 0 } });
        }
        node = node.dirs.get(part);
        node.totals.files += item.path ? 1 : 0;
        node.totals.additions += item.additions || 0;
        node.totals.deletions += item.deletions || 0;
      });
      node.files.push(item);
    });
    return root;
  }

  function renderTree(node, kind, depth) {
    depth = depth || 0;
    var rows = [];
    Array.from(node.dirs.values()).sort(byName).forEach(function (dir) {
      var collapsed = state.collapsed.has(kind + ":" + dir.path);
      var stats = dir.totals.files ? dir.totals.files + " +" + dir.totals.additions + "/-" + dir.totals.deletions : "";
      rows.push(row({
        action: "toggle",
        key: "",
        id: kind + ":" + dir.path,
        main: treeMain(depth, collapsed ? ">" : "v", dir.name + "/", ""),
        detail: stats
      }));
      if (!collapsed) rows = rows.concat(renderTree(dir, kind, depth + 1));
    });
    node.files.sort(function (a, b) { return changePath(a).localeCompare(changePath(b)); }).forEach(function (item) {
      var path = changePath(item);
      var action = kind === "change" ? "diff" : "file";
      var detail = item.additions || item.deletions ? "+" + (item.additions || 0) + "/-" + (item.deletions || 0) : "";
      rows.push(row({
        action: action,
        key: keyFor(action, path),
        id: path,
        main: treeMain(depth, markerFor(item), basename(path), item.kind),
        detail: detail
      }));
    });
    return rows;
  }

  function treeMain(depth, marker, name, markerClass) {
    return '<span class="tree-line"><span class="tree-name"><span class="indent" style="width:' + (depth * 18) + 'px"></span>' +
      '<span class="marker ' + escapeHtml(markerClass || "") + '">' + escapeHtml(marker) + '</span>' +
      escapeHtml(name) + '</span><span></span></span>';
  }

  function markerFor(change) {
    if (!change.kind) return "";
    if (change.kind === "modified") return ".";
    if (change.kind === "added" || change.kind === "untracked") return "+";
    if (change.kind === "deleted") return "-";
    if (change.kind === "conflict") return "!";
    if (change.staged && change.unstaged) return "S+";
    if (change.staged) return "S";
    return ".";
  }

  function renderGit() {
    var git = state.snapshot.git || {};
    var rows = [];
    rows.push(section("Summary", [
      row({
        action: "git_summary",
        key: keyFor("git_summary", "summary"),
        id: "summary",
        main: escapeHtml(git.current_branch || "HEAD"),
        detail: "up " + (git.ahead || 0) + " down " + (git.behind || 0)
      }),
      row({
        action: "git_summary",
        key: "",
        id: "summary",
        main: escapeHtml("base: " + (git.base_branch || "none")),
        detail: git.upstream ? "upstream " + git.upstream : ""
      })
    ]));
    rows.push(section("Branches", (git.branches || []).map(function (branch) {
      return row({
        action: "git_branch",
        key: keyFor("git_branch", branch.name),
        id: branch.name,
        main: escapeHtml(branch.name),
        detail: branch.is_current ? "current" : (branch.is_remote ? "remote" : "local")
      });
    })));
    rows.push(section("Recent commits", (git.recent_commits || []).map(function (commit) {
      return row({
        action: "git_commit",
        key: keyFor("git_commit", commit.sha),
        id: commit.sha,
        main: escapeHtml(commit.short_sha + "  " + commit.summary),
        detail: commit.date + " " + commit.author
      });
    })));
    rows.push(section("Stashes", (git.stashes || []).map(function (stash) {
      return row({
        action: "git_stash",
        key: keyFor("git_stash", stash.name),
        id: stash.name,
        main: escapeHtml(stash.name + "  " + stash.summary),
        detail: ""
      });
    })));
    rows.push(section("Tags", (git.tags || []).map(function (tag) {
      return row({
        action: "git_tag",
        key: keyFor("git_tag", tag.name),
        id: tag.name,
        main: escapeHtml(tag.name),
        detail: ""
      });
    })));
    rows.push(section("Remotes", (git.remotes || []).map(function (remote) {
      return row({
        action: "git_remote",
        key: keyFor("git_remote", remote.name),
        id: remote.name,
        main: escapeHtml(remote.name + " fetch " + remote.fetch_url),
        detail: remote.push_url ? "push" : ""
      });
    })));
    listPane.innerHTML = rows.join("");
    bindRows();
  }

  function renderIssues() {
    var issues = state.snapshot.issues || [];
    listPane.innerHTML = section("Issues", issues.map(function (issue) {
      return row({
        action: "",
        key: keyFor("issue", issue.key),
        id: issue.key,
        main: escapeHtml(issue.key + "  " + issue.title),
        detail: issue.status + " " + issue.priority
      });
    })) || '<div class="empty">No issues</div>';
  }

  function renderAgents() {
    var agents = state.snapshot.agents || [];
    listPane.innerHTML = section("Agents", agents.map(function (agent) {
      return row({
        action: "",
        key: keyFor("agent", agent.id),
        id: agent.id,
        main: escapeHtml(agent.id + "  " + agent.title),
        detail: agent.status || agent.agent || ""
      });
    })) || '<div class="empty">No agents</div>';
  }

  function renderSearch() {
    listPane.innerHTML = '<input class="search-box" id="searchBox" placeholder="Search files, changes, git, issues, agents" value="' + escapeHtml(state.searchQuery) + '">' +
      section("Results", state.searchResults.map(function (result) {
        return row({
          action: searchAction(result.target),
          key: keyFor("search", result.label),
          id: searchId(result.target),
          main: escapeHtml(result.label),
          detail: result.detail
        });
      }));
    var input = document.getElementById("searchBox");
    input.focus();
    input.oninput = debounce(function () {
      state.searchQuery = input.value;
      fetchSearch();
    }, 160);
    bindRows();
  }

  function searchAction(target) {
    if (!target) return "";
    if (target.kind === "file") return "file";
    if (target.kind === "change") return "diff";
    if (target.kind === "git_commit") return "git_commit";
    if (target.kind === "git_branch") return "git_branch";
    if (target.kind === "git_stash") return "git_stash";
    if (target.kind === "git_tag") return "git_tag";
    return "";
  }

  function searchId(target) {
    if (!target) return "";
    return target.path || target.sha || target.name || target.key || target.id || "";
  }

  function loadPreview(kind, id) {
    var params = new URLSearchParams({ kind: kind });
    if (kind === "diff" || kind === "file") params.set("path", id);
    if (kind === "git_commit") params.set("sha", id);
    if (kind === "git_branch") params.set("branch", id);
    if (kind === "git_stash") params.set("stash", id);
    if (kind === "git_tag" || kind === "git_remote") params.set("name", id);
    if (kind === "git_summary") params.set("name", id);
    previewPane.innerHTML = '<div class="empty">Loading preview</div>';
    fetch("/api/preview?" + params.toString())
      .then(check)
      .then(function (preview) {
        renderPreview(preview);
      })
      .catch(function (error) {
        previewPane.innerHTML = '<div class="empty">' + escapeHtml(error.message) + '</div>';
      });
  }

  function renderPreview(preview) {
    var lines = text(preview.content).split("\n");
    var body = lines.map(function (line) {
      var cls = "code-line";
      if (line.startsWith("+") && !line.startsWith("+++")) cls += " add";
      if (line.startsWith("-") && !line.startsWith("---")) cls += " del";
      if (line.startsWith("@@")) cls += " hunk";
      return '<span class="' + cls + '">' + window.WorkdeckHighlight.highlight(line, preview.title) + "</span>";
    }).join("");
    previewPane.innerHTML = '<div class="preview-header"><div class="preview-title">' + escapeHtml(preview.title) + '</div>' +
      '<div class="preview-meta">' + (preview.truncated ? "truncated" : "") + '</div></div><pre>' + body + "</pre>";
  }

  function fetchSnapshot() {
    fetch("/api/snapshot")
      .then(check)
      .then(function (snapshot) {
        state.snapshot = snapshot;
        render();
      })
      .catch(function (error) {
        listPane.innerHTML = '<div class="empty">' + escapeHtml(error.message) + '</div>';
      });
  }

  function fetchSearch() {
    if (!state.searchQuery.trim()) {
      state.searchResults = [];
      render();
      return;
    }
    fetch("/api/search?q=" + encodeURIComponent(state.searchQuery))
      .then(check)
      .then(function (payload) {
        state.searchResults = payload.results || [];
        render();
      });
  }

  function connectEvents() {
    if (!window.EventSource) return;
    var source = new EventSource("/events");
    source.addEventListener("snapshot_updated", function () {
      fetchSnapshot();
    });
  }

  function check(response) {
    if (!response.ok) {
      return response.json().then(function (payload) {
        throw new Error(payload.error || response.statusText);
      });
    }
    return response.json();
  }

  function changePath(item) {
    return item.path || "";
  }

  function basename(path) {
    var parts = path.split("/");
    return parts[parts.length - 1] || path;
  }

  function byName(a, b) {
    return a.name.localeCompare(b.name);
  }

  function escapeHtml(value) {
    return window.WorkdeckHighlight.escape(value);
  }

  function debounce(fn, ms) {
    var timer = null;
    return function () {
      clearTimeout(timer);
      timer = setTimeout(fn, ms);
    };
  }

  fetchSnapshot();
  connectEvents();
})();
