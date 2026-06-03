#[cfg(test)]
mod tests {
    use crate::model::res::{FILE_BLOBS, FILES, PAGE_BODIES, PAGES};
    use crate::model::user::USERS;

    #[test]
    fn init_database() {
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
    }
}
