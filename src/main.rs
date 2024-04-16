use diesel::{r2d2::ConnectionManager, MysqlConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use glob::glob;
use irc::client::reactor::IrcReactor;

use frippy::plugins::counter::Counter;
use frippy::plugins::factoid::Factoid;
use frippy::plugins::help::Help;
use frippy::plugins::keepnick::KeepNick;
use frippy::plugins::quote::Quote;
use frippy::plugins::remind::Remind;
use frippy::plugins::sed::Sed;
use frippy::plugins::tell::Tell;
use frippy::plugins::unicode::Unicode;
use frippy::plugins::url::UrlTitles;

use failure::{bail, format_err, Error};
use frippy::Config;
use log::{error, info};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

fn main() {
    if let Err(e) = log4rs::init_file("log.yml", Default::default()) {
        use log4rs::Error;
        match e {
            Error::Log(e) => eprintln!("Log4rs error: {}", e),
            Error::Log4rs(e) => eprintln!("Failed to parse \"log.yml\" as log4rs config: {}", e),
        }

        return;
    }

    // Print any errors that caused frippy to shut down
    if let Err(e) = run() {
        let text = e
            .iter_causes()
            .fold(format!("{}", e), |acc, err| format!("{}: {}", acc, err));
        error!("{}", text);
    }
}

fn run() -> Result<(), Error> {
    // Load all toml files in the configs directory
    let mut configs = Vec::new();
    for toml in glob("configs/*.toml").unwrap() {
        match toml {
            Ok(path) => {
                info!("Loading {}", path.to_str().unwrap());
                match Config::load(path) {
                    Ok(v) => configs.push(v),
                    Err(e) => error!("Incorrect config file {}", e),
                }
            }
            Err(e) => error!("Failed to read path {}", e),
        }
    }

    // Without configs the bot would just idle
    if configs.is_empty() {
        bail!("No config file was found");
    }

    // Create an event loop to run the connections on.
    let mut reactor = IrcReactor::new()?;

    // Open a connection and add work for each config
    for config in configs {
        let mut disabled_plugins = None;
        let (mysql_url, prefix) = if let Some(ref options) = config.options {
            if let Some(disabled) = options.get("disabled_plugins") {
                disabled_plugins = Some(disabled.split(',').map(|p| p.trim()).collect::<Vec<_>>());
            }
            let prefix = options.get("prefix");

            let url = options
                .get("mysql_url")
                .ok_or(format_err!("Must set mysql_url"))?;

            (url, prefix)
        } else {
            bail!("Must set mysql_url option");
        };

        let prefix = prefix.cloned().unwrap_or_else(|| String::from("."));

        let mut bot = frippy::Bot::new(&prefix);
        bot.add_plugin(Help::new());
        bot.add_plugin(UrlTitles::new(1024));
        bot.add_plugin(Sed::new(60));
        bot.add_plugin(Unicode::new());
        bot.add_plugin(KeepNick::new());

        {
            let manager = ConnectionManager::<MysqlConnection>::new(mysql_url);
            match r2d2::Pool::builder().build(manager) {
                Ok(pool) => {
                    if let Err(e) = pool.get()?.run_pending_migrations(MIGRATIONS) {
                        bail!("Failed to run migrations: {}", e);
                    }

                    bot.add_plugin(Factoid::new(pool.clone()));
                    bot.add_plugin(Quote::new(pool.clone()));
                    bot.add_plugin(Tell::new(pool.clone()));
                    bot.add_plugin(Remind::new(pool.clone()));
                    bot.add_plugin(Counter::new(pool.clone()));

                    info!("Connected to MySQL server")
                }
                Err(e) => bail!("Failed to connect to database: {}", e),
            }
        }

        if let Some(disabled_plugins) = disabled_plugins {
            for name in disabled_plugins {
                if bot.remove_plugin(name).is_none() {
                    error!("\"{}\" was not found - could not disable", name);
                }
            }
        }

        bot.connect(&mut reactor, &config)?;
    }

    // Run the bots until they throw an error - an error could be loss of connection
    reactor.run()?;

    Ok(())
}
