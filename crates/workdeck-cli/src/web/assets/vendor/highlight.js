(function () {
  function escapeHtml(value) {
    return String(value)
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;");
  }

  var keywords = /\b(async|await|break|case|class|const|continue|computed|def|else|enum|export|fn|for|from|function|if|impl|import|interface|let|match|mod|props|pub|reactive|ref|return|setup|struct|trait|use|var|while)\b/g;
  var strings = /("[^"]*"|&quot;[^&]*?&quot;|'[^']*?')/g;
  var comments = /(\/\/.*|#.*)$/;

  function languageForPath(path) {
    var clean = String(path || "").split("?")[0].toLowerCase();
    var ext = clean.includes(".") ? clean.split(".").pop() : "";
    if (ext === "vue") return "vue";
    if (ext === "html" || ext === "htm") return "html";
    if (ext === "css" || ext === "scss" || ext === "sass") return "css";
    if (ext === "js" || ext === "jsx" || ext === "ts" || ext === "tsx" || ext === "mjs" || ext === "cjs") return "js";
    return "plain";
  }

  function highlightGeneric(value) {
    return escapeHtml(value)
      .replace(strings, '<span class="hl-string">$1</span>')
      .replace(keywords, '<span class="hl-keyword">$1</span>')
      .replace(comments, '<span class="hl-comment">$1</span>');
  }

  function highlightMarkup(value) {
    return escapeHtml(value)
      .replace(/(&lt;!--.*?--&gt;)/g, '<span class="hl-comment">$1</span>')
      .replace(/(\s)([@:#]?[A-Za-z_][\w:.-]*)(=)/g, '$1<span class="hl-attr">$2</span>$3')
      .replace(/(&lt;\/?)([A-Za-z][\w:-]*)/g, '$1<span class="hl-tag">$2</span>')
      .replace(strings, '<span class="hl-string">$1</span>');
  }

  function highlightCss(value) {
    return escapeHtml(value)
      .replace(/^([\s]*[.#]?[A-Za-z_-][\w-]*)(\s*\{)/, '<span class="hl-selector">$1</span>$2')
      .replace(/^([\s-]*[A-Za-z-]+)(\s*:)/, '<span class="hl-prop">$1</span>$2')
      .replace(/\b([0-9]+(?:\.[0-9]+)?(?:px|rem|em|vh|vw|%)?)\b/g, '<span class="hl-number">$1</span>')
      .replace(strings, '<span class="hl-string">$1</span>')
      .replace(/(\/\*.*?\*\/)/g, '<span class="hl-comment">$1</span>');
  }

  function highlightVue(value) {
    var trimmed = String(value).trimStart();
    if (trimmed.startsWith("<") || trimmed.startsWith("@") || trimmed.startsWith(":") || trimmed.startsWith("v-")) {
      return highlightMarkup(value);
    }
    if (/^[\s.#]?[A-Za-z_-][\w-]*(\s*\{|\s*:)/.test(value)) {
      return highlightCss(value);
    }
    return highlightGeneric(value);
  }

  window.WorkdeckHighlight = {
    escape: escapeHtml,
    languageForPath: languageForPath,
    highlight: function (value, path) {
      var language = languageForPath(path);
      if (language === "vue") return highlightVue(value);
      if (language === "html") return highlightMarkup(value);
      if (language === "css") return highlightCss(value);
      return highlightGeneric(value);
    }
  };
})();
