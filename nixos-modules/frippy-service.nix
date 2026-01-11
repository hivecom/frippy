{
  config,
  pkgs,
  lib ? pkgs.lib,
  ...
}:
with lib;
let
  cfg = config.services.frippy;
in
{

  ###### interface
  options = {
    services.frippy = {
      enable = mkOption {
        type = types.bool;
        default = false;
        description = ''
          Whether to run frippy
        '';
      };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.frippy;
        defaultText = "pkgs.frippy";
        description = "Frippy package";
      };
      owners = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = ''
          Usernames of the owners of this bot.
        '';
      };
      prefix = mkOption {
        type = types.str;
        default = ".";
        description = ''
          Prefix for commands.
        '';
      };
      disabledPlugins = mkOption {
        type = types.str;
        default = "Url";
        description = ''
          Plugins to disable.
        '';
      };

      logLevel = mkOption {
        type = types.str;
        default = "debug";
        description = ''
          Log level.
        '';
      };

      user = {
        nickname = mkOption {
          type = types.str;
          default = "frippy";
          description = ''
            Nickname of the bot.
          '';
        };
        password = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = ''
            Password for the bot if it has an account.
          '';
        };
        altNicks = mkOption {
          type = types.listOf types.str;
          default = [ ];
          description = ''
            Fallback nicknames for the bot.
          '';
        };
        realname = mkOption {
          type = types.str;
          default = "frippy";
          description = ''
            Realname of the bot.
          '';
        };
        umodes = mkOption {
          type = types.str;
          default = "+B";
          description = ''
            Umodes for the user.
          '';
        };
        userInfo = mkOption {
          type = types.str;
          default = "IRC Bot";
          description = ''
            Self description of the bot.
          '';
        };
        version = mkOption {
          type = types.str;
          default = "frippy v0.5.1";
          description = ''
            Response to CTCP version command.
          '';
        };
        source = mkOption {
          type = types.str;
          default = "https://github.com/hivecom/frippy";
          description = ''
            Response to CTCP source command.
          '';
        };
      };

      server = {
        address = mkOption {
          type = types.str;
          description = ''
            Address of the IRC server to connect to.
          '';
        };
        password = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = ''
            Optional password of the IRC server.
          '';
        };
        port = mkOption {
          type = types.int;
          default = 6697;
          description = ''
            Port of the IRC server.
          '';
        };
        ssl = mkOption {
          type = types.bool;
          default = true;
          description = ''
            Whether to connect via SSL.
          '';
        };
        encoding = mkOption {
          type = types.str;
          default = "UTF-8";
          description = ''
            Encoding to use for this server.
          '';
        };
        channels = mkOption {
          type = types.listOf types.str;
          default = "";
          description = ''
            List of channels to connect to automatically.
          '';
        };
        channelKeys = mkOption {
          type = types.attrsOf types.str;
          default = { };
          description = ''
            Passwords for the channels that need it.
          '';
        };
      };
      bridge = {
        name = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = ''
            Name of the bridge bot.
          '';
        };
        regex = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = ''
            How username and message should be extracted - uses https://docs.rs/regex/ syntax
          '';
        };
        relayFormat = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = ''
            If the bridge uses IRCv3 relay nicks this can extract the username - uses https://docs.rs/regex/ syntax
          '';
        };
        ignoreRegex = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = ''
            Part of messages to remove (like "edited" suffixes for example)
          '';
        };
        removeZWS = mkOption {
          type = types.nullOr types.bool;
          default = null;
          description = ''
            Whether to remove ZWS spaces from usernames.
          '';
        };
      };
      database = {
        enable = mkEnableOption "the mysql database for use with frippy. See {option}`services.mysql`" // {
          default = true;
        };
        createDB = mkEnableOption "the automatic creation of the database for frippy." // {
          default = true;
        };
        name = mkOption {
          type = types.str;
          default = "frippy";
          description = "The name of the frippy database.";
        };
        host = mkOption {
          type = types.str;
          default = "localhost";
          description = "Host for the mysql server.";
        };
        port = mkOption {
          type = types.port;
          default = 3306;
          description = "Port of the mysql server.";
        };
        user = mkOption {
          type = types.str;
          default = "frippy";
          description = "The database user for frippy.";
        };
        password = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = "The password for the database user for frippy.";
        };
      };
    };
  };

  ###### implementation

  config = mkIf cfg.enable {
    systemd.services.frippy = {
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      description = "IRC Bot written in Rust";
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package}";
        Restart = "always";
        RestartSec = 30;
        WorkingDirectory = "/etc/frippy";

        DynamicUser = true;
        StateDirectory = "frippy";
        LockPersonality = true;
        ProtectSystem = true;
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        RemoveIPC = true;
        RestrictAddressFamilies = [ ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
      };
    };
    # FIXME: security implications of being in the nix store?
    environment.etc."frippy/configs/config.toml".source =
      lib.pipe
        {
          inherit (cfg) owners;
          inherit (cfg.user) nickname;
          nick_password = cfg.user.password;
          alt_nicks = cfg.user.altNicks;
          inherit (cfg.user) realname;
          server = cfg.server.address;
          inherit (cfg.server) password;
          inherit (cfg.server) port;
          use_ssl = cfg.server.ssl;
          inherit (cfg.server) encoding;
          inherit (cfg.server) channels;
          inherit (cfg.user) umodes;
          user_info = cfg.user.userInfo;
          inherit (cfg.user) version;
          inherit (cfg.user) source;
          # FIXME: merge with channels into a { name, key } list
          channel_keys = cfg.server.channelKeys;

          options = {
            inherit (cfg) prefix;
            disabled_plugins = cfg.disabledPlugins;
            mysql_url =
              if cfg.database.password != null then
                "mysql://${cfg.database.user}:${cfg.database.password}@${cfg.database.host}:${toString cfg.database.port}/${cfg.database.name}"
              else
                "mysql://${cfg.database.user}@${cfg.database.host}:${toString cfg.database.port}/${cfg.database.name}";

            bridge_name = cfg.bridge.name;
            bridge_regex = cfg.bridge.regex;
            bridge_relay_format = cfg.bridge.relayFormat;
            bridge_ignore_regex = cfg.bridge.ignoreRegex;
            bridge_remove_zws = toString cfg.bridge.removeZWS;
          };
        }
        [
          (lib.filterAttrsRecursive (_k: v: v != null))
          ((pkgs.formats.toml { }).generate "frippy-config")
        ];

    environment.etc."frippy/log.yml".source = (pkgs.formats.yaml { }).generate "frippy-log-config" {
      appenders.stdout = {
        kind = "console";
        encoder = {
          kind = "pattern";
          pattern = "[{d:35}](({h({l})})) {t} - {m}{n}";
        };
      };
      loggers.frippy = {
        level = cfg.logLevel;
        appenders = [ "stdout" ];
      };
    };

    services.mysql = mkIf cfg.database.enable {
      enable = true;
      package = pkgs.mariadb;
      ensureDatabases = mkIf cfg.database.createDB [ cfg.database.name ];
      ensureUsers = mkIf cfg.database.createDB [
        {
          name = cfg.database.user;
          ensurePermissions = {
            "${cfg.database.name}.*" = "ALL PRIVILEGES";
          };
        }
      ];
    };
  };
}
