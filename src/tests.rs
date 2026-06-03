#[cfg(test)]
mod tests {
    use crate::model::res::{FILES, PAGES};
    use crate::model::user::USERS;
    // use redb::ReadableDatabase;

    #[test]
    fn init_database() {
        let db = redb::Database::create("data.redb").unwrap();

        let tx = db.begin_write().unwrap();
        {
            tx.open_table(FILES).unwrap();
            tx.open_table(PAGES).unwrap();
            tx.open_table(USERS).unwrap();
        }
        tx.commit().unwrap();
    }
}
