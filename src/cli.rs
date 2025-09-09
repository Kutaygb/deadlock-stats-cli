use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "deadlock-cli", version, about = "Deadlock stats CLI")] 
pub struct Args {
    #[arg(long, global = true, help = "Output raw JSON instead of tables")]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    #[command(alias = "by-steamid")]
    BySteamId {
        /// 17-digit steamID64
        #[arg(long = "id")]
        id: String,
    },

    #[command(alias = "by-steamid3")]
    BySteamId3 {
        /// exmp [U:1:123456789] or 388674065
        #[arg(long = "id3")]
        id3: String,
    },

    ByVanity {
        #[arg(long = "name")]
        name: String,
    },

    ByUrl {
        #[arg(long = "url")]
        url: String,
    },

    Migrate,

    Matches {
        #[command(subcommand)]
        cmd: MatchesSubcommand,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum MatchesSubcommand {
    Sync {
        #[arg(long = "id", value_delimiter = ',')]
        ids: Vec<i64>,

        #[arg(long = "from-account-id")]
        from_account_id: Option<u32>,

        #[arg(long = "from-steamid")] 
        from_steamid: Option<String>,

        #[arg(long = "from-id3")]
        from_id3: Option<String>,

        #[arg(long = "since-id")]
        since_id: Option<i64>,

        #[arg(long = "until-id")]
        until_id: Option<i64>,

        #[arg(long, default_value_t = 500)]
        limit: usize,

        #[arg(long = "batch-size", default_value_t = 100)]
        batch_size: usize,

        #[arg(long = "include-info", default_value_t = true)]
        include_info: bool,

        #[arg(long = "include-players", default_value_t = true)]
        include_players: bool,

        #[arg(long = "dry-run", default_value_t = false)]
        dry_run: bool,
    },

    History {
        #[arg(long = "account-id")]
        account_id: Option<u32>,

        #[arg(long = "steamid")]
        steamid: Option<String>,

        #[arg(long = "id3")]
        id3: Option<String>,

        #[arg(long = "force-refetch", default_value_t = false)]
        force_refetch: bool,

        #[arg(long = "only-stored-history", default_value_t = false)]
        only_stored_history: bool,

        #[arg(long = "dry-run", default_value_t = false)]
        dry_run: bool,
    },
}
