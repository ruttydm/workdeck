{
  config,
  lib,
  pkgs,
  ...
}:
with lib;
let
  cfg = config.programs.workdeck;
  tomlFormat = pkgs.formats.toml {};
in {
  options.programs.workdeck = {
    enable = mkEnableOption "Workdeck, a terminal-native review and repository workbench";

    package = mkOption {
      type = types.package;
      default = pkgs.workdeck;
      defaultText = literalExpression "pkgs.workdeck";
      description = "The Workdeck package to use.";
    };

    settings = mkOption {
      type = tomlFormat.type;
      default = {};
      example = literalExpression ''
        {
          ui.theme = "graphite";
          review = {
            mode = "split";
            line_numbers = true;
            exclude_untracked = false;
            tab_width = 4;
          };
        }
      '';
      description = "Configuration written to workdeck/config.toml.";
    };

    enableGitIntegration = mkOption {
      type = types.bool;
      default = false;
      description = "Whether to set Workdeck as the default Git pager.";
    };

    enableJujutsuIntegration = mkOption {
      type = types.bool;
      default = false;
      description = "Whether to set Workdeck as the Jujutsu pager and request Git-format diffs.";
    };

    enableClaudeIntegration = mkOption {
      type = types.bool;
      default = false;
      description = "Whether to link the workdeck-review skill under ~/.claude/skills.";
    };
  };

  config = mkIf cfg.enable {
    home.packages = [cfg.package];

    xdg.configFile."workdeck/config.toml" = mkIf (cfg.settings != {}) {
      source = tomlFormat.generate "workdeck-config.toml" cfg.settings;
    };

    programs.git.settings.core.pager = mkIf cfg.enableGitIntegration "workdeck pager";

    programs.jujutsu.settings = mkIf cfg.enableJujutsuIntegration {
      ui = {
        diff-formatter = ":git";
        pager = "workdeck pager";
      };
    };

    home.file = mkIf cfg.enableClaudeIntegration {
      ".claude/skills/workdeck-review".source = "${cfg.package}/share/workdeck/skills/workdeck-review";
    };
  };
}
