use book::crypto::Signed;
use book::model::{ENTRIES, ENTRY_BODY, FILE_BLOB, FILES, USERS};
use book::model::{Passkey, Slug, User};
use clap::Parser;
use time::OffsetDateTime;
use time::format_description::well_known::Iso8601;

#[derive(Parser)]
#[command(name = "cli")]
enum Cli {
    InitTables(InitTables),
    InitUser(InitUser),
    GenPasskey(GenPasskey),
}

fn main() {
    match Cli::parse() {
        Cli::InitTables(cmd) => cmd.run(),
        Cli::InitUser(cmd) => cmd.run(),
        Cli::GenPasskey(cmd) => cmd.run(),
    }
}

// init tables
//
// ++++++++++++============++++++++++++============++++++++++++============

#[derive(clap::Args)]
/// Initialize database tables
struct InitTables;

impl InitTables {
    /// run the command
    fn run(&self) {
        let db = redb::Database::create("data.redb").unwrap();

        let tx = db.begin_write().unwrap();
        {
            tx.open_table(ENTRIES).unwrap();
            tx.open_table(ENTRY_BODY).unwrap();
            tx.open_table(FILES).unwrap();
            tx.open_table(FILE_BLOB).unwrap();
            tx.open_table(USERS).unwrap();
        }
        tx.commit().unwrap();

        println!("tables initialized");
    }
}

// init user
//
// ++++++++++++============++++++++++++============++++++++++++============

#[derive(clap::Args)]
/// Create a new user
struct InitUser {
    /// Username for the new user
    username: String,
    /// Password for the new user
    password: String,
}

impl InitUser {
    /// run the command
    fn run(&self) {
        let db = redb::Database::create("data.redb").unwrap();

        let tx = db.begin_write().unwrap();
        {
            let mut users = tx.open_table(USERS).unwrap();
            let user = User::new(&self.password, &book::CONFIG.secret, &self.username);
            let username = Slug::new(self.username.clone()).expect("invalid username");
            users.insert(username, user).unwrap();
        }
        tx.commit().unwrap();

        println!("user created: {}", self.username);
    }
}

// gen passkey
//
// ++++++++++++============++++++++++++============++++++++++++============

#[derive(clap::Args)]
/// Generate a passkey
struct GenPasskey;

impl GenPasskey {
    /// run the command
    fn run(&self) {
        gen_passkey("")
    }
}

fn gen_passkey(creator: &str) {
    let passkey = Passkey::new(creator);
    let signed = Signed::new(passkey.clone());
    let code = signed.generate(&book::CONFIG.secret);

    let expires_at = OffsetDateTime::from_unix_timestamp(passkey.expires_at)
        .ok()
        .and_then(|d| d.format(&Iso8601::DATE).ok())
        .unwrap_or_default();

    let url = format!("{}/auth?passkey={}", book::CONFIG.base_url, code);
    println!("Passkey for '{}':", creator);
    println!("  Code:    {}", &code[..32]);
    println!("  URL:     {url}");
    println!("  Expires: {expires_at}");
}
