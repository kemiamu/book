use book::model::user::User;
use book::model::{FILE_BLOB, FILES, PAGE_HTML, PAGE_RAW, PAGES, USERS};
use clap::Parser;

#[derive(Parser)]
#[command(name = "cli")]
enum Cli {
    /// Initialize database tables
    InitTables,
    /// Create a new user
    InitUser {
        /// Username for the new user
        username: String,
        /// Password for the new user
        password: String,
    },
}

fn main() {
    match Cli::parse() {
        Cli::InitTables => init_tables(),
        Cli::InitUser { username, password } => init_user(&username, &password),
    }
}

fn init_tables() {
    let db = redb::Database::create("data.redb").unwrap();

    let tx = db.begin_write().unwrap();
    {
        tx.open_table(PAGES).unwrap();
        tx.open_table(PAGE_RAW).unwrap();
        tx.open_table(PAGE_HTML).unwrap();
        tx.open_table(FILES).unwrap();
        tx.open_table(FILE_BLOB).unwrap();
        tx.open_table(USERS).unwrap();
    }
    tx.commit().unwrap();

    println!("tables initialized");
}

fn init_user(username: &str, password: &str) {
    let db = redb::Database::create("data.redb").unwrap();

    let tx = db.begin_write().unwrap();
    {
        let mut users = tx.open_table(USERS).unwrap();
        let user = User::new(password, &book::CONFIG.secret, username);
        users.insert(username, user).unwrap();
    }
    tx.commit().unwrap();

    println!("user created: {username}");
}
