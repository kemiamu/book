#[cfg(test)]
mod tests {
    use crate::model::res::{FILE_BLOBS, FILES, PAGE_BODIES, PAGES};
    use crate::model::user::{USERS, User};

    #[test]
    fn init_tables() {
        let db = redb::Database::create("data.redb").unwrap();

        let tx = db.begin_write().unwrap();
        {
            tx.open_table(PAGES).unwrap();
            tx.open_table(PAGE_BODIES).unwrap();
            tx.open_table(FILES).unwrap();
            tx.open_table(FILE_BLOBS).unwrap();
            tx.open_table(USERS).unwrap();
        }
        tx.commit().unwrap();

        println!("tables initialized");
    }

    #[test]
    fn init_user() {
        let mut args = std::env::args().skip_while(|a| a != "init_user");
        args.next();

        let (username, password) = match (args.next(), args.next()) {
            (Some(u), Some(p)) => (u, p),
            _ => panic!("usage: cargo test init_user -- <username> <password>"),
        };

        let db = redb::Database::create("data.redb").unwrap();

        let tx = db.begin_write().unwrap();
        {
            let mut users = tx.open_table(USERS).unwrap();
            let user = User::new(&password, &crate::CONFIG.secret, &username);
            users.insert(username.as_str(), user).unwrap();
        }
        tx.commit().unwrap();

        println!("user created: {username}");
    }
}
