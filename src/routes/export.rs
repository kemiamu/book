// site export
//
// ++++++++++++============++++++++++++============++++++++++++============

use axum::body::{Body, Bytes};
use axum::response::Response;
use axum::{extract::State, http::StatusCode};
use book::CONFIG;
use book::model::{AppState, ENTRIES, ENTRY_BODY, EntryId, EntryMeta};
use book::model::{FILE_BLOB, FILES, FileMeta, FileName, Slug};
use redb::ReadableDatabase;
use std::{collections::BTreeMap, error::Error, io, sync::Arc};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// stream the whole site into a zip archive, mirroring the site urls
pub async fn export_zip(State(state): State<Arc<AppState>>) -> Response {
    let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(16);
    tokio::task::spawn_blocking(move || {
        if let Err(e) = write_zip(&state.db, ZipSink(tx)) {
            tracing::error!("site export failed: {e}");
        }
    });

    let date = super::format_date(time::UtcDateTime::now().unix_timestamp());
    let filename = format!("book-export-{date}.zip");
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/zip")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from_stream(ReceiverStream::new(rx)))
        .unwrap()
}

/// write every entry and file into the zip
fn write_zip<W: io::Write, D: ReadableDatabase>(db: &D, sink: W) -> Result<(), Box<dyn Error>> {
    use redb::ReadableTable;
    use std::io::Write;

    let tx = db.begin_read()?;

    // entries sorted by numeric id
    let mut entries: Vec<(u64, EntryId, EntryMeta)> = Vec::new();
    for result in tx.open_table(ENTRIES)?.iter()? {
        let (key, value) = result?;
        let id = key.value();
        entries.push((u64::from_str_radix(id.as_ref(), 16)?, id, value.value()));
    }
    entries.sort_by_key(|(num, _, _)| *num);

    // files with their metadata
    let mut files: Vec<(EntryId, Slug<FileName>, FileMeta)> = Vec::new();
    for result in tx.open_table(FILES)?.iter()? {
        let (key, value) = result?;
        let (entry_id, file_name) = key.value();
        files.push((entry_id, file_name, value.value()));
    }

    let text = SimpleFileOptions::default();
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut zip = ZipWriter::new_stream(sink);
    let body_table = tx.open_table(ENTRY_BODY)?;

    // raw markdown with a generated top-level title, mirroring the site url
    for (_, id, meta) in &entries {
        let Some(guard) = body_table.get(id)? else {
            return Err(format!("entry body missing: {id}").into());
        };
        let body = guard.value();
        let title = escape_md(&meta.title);
        zip.start_file(format!("{id}/README.md"), text)?;
        write!(zip, "# {title}\n\n")?;
        zip.write_all(body.raw().as_bytes())?;
    }

    // attached files, stored uncompressed
    let blob_table = tx.open_table(FILE_BLOB)?;
    for (entry_id, file_name, _) in &files {
        let key = (entry_id.clone(), file_name.clone());
        let Some(data) = blob_table.get(&key)? else {
            continue;
        };
        zip.start_file(format!("{entry_id}/file/{file_name}"), stored)?;
        zip.write_all(data.value())?;
    }

    // root index: entries grouped by category, newest first per group
    let mut sections: BTreeMap<String, Vec<&(u64, EntryId, EntryMeta)>> = BTreeMap::new();
    for entry in &entries {
        sections
            .entry(entry.2.category.clone())
            .or_default()
            .push(entry);
    }
    let site_title = escape_md(&CONFIG.site_title);
    let mut index = format!("# {site_title}\n");
    for (category, mut group) in sections {
        group.sort_by(|a, b| b.2.last_modified.cmp(&a.2.last_modified));
        let heading = if category.is_empty() {
            "Uncategorized"
        } else {
            &category
        };
        let heading = escape_md(heading);
        index.push_str(&format!("\n## {heading}\n\n"));
        for (_, id, meta) in group {
            let title = escape_md(&meta.title);
            index.push_str(&format!("- [{title}]({id}/README.md)\n"));
        }
    }
    zip.start_file("README.md", text)?;
    zip.write_all(index.as_bytes())?;

    zip.finish()?;
    Ok(())
}

/// escape markdown special characters in headings and link labels
fn escape_md(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '#' | '|' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// io::Write adapter forwarding zip chunks into the response body channel
struct ZipSink(mpsc::Sender<Result<Bytes, io::Error>>);

impl io::Write for ZipSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for chunk in buf.chunks(64 * 1024) {
            self.0
                .blocking_send(Ok(Bytes::copy_from_slice(chunk)))
                .map_err(|_| io::Error::other("client disconnected"))?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// open the real database read-only; tests run in parallel, and redb
    /// locks the file, so the open is serialized and the guard is held
    /// until the database is dropped
    fn open_test_db() -> (redb::ReadOnlyDatabase, std::sync::MutexGuard<'static, ()>) {
        static DB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = DB_LOCK.lock().unwrap();
        let db = redb::Database::builder()
            .open_read_only("data.redb")
            .expect("open data.redb");
        (db, guard)
    }

    /// export the real database and read the archive back, verifying every entry
    #[test]
    fn export_roundtrip() {
        use std::io::Read;
        let (db, _guard) = open_test_db();
        let mut buf = Vec::new();
        write_zip(&db, &mut buf).expect("write zip");

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&buf)).expect("parse zip");
        let names: Vec<String> = archive.file_names().map(String::from).collect();
        assert!(names.contains(&"README.md".to_string()));
        assert!(names.iter().any(|n| n.ends_with("/README.md")));

        // every entry body starts with the generated top-level title
        let first_name = names
            .iter()
            .find(|n| n.ends_with("/README.md"))
            .expect("entry readme");
        let mut first = archive.by_name(first_name).expect("open entry readme");
        let mut head = String::new();
        first.read_to_string(&mut head).expect("read head");
        assert!(head.starts_with("# "));
        drop(first);

        // reading every entry verifies its crc and size
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).expect("open entry");
            std::io::copy(&mut file, &mut std::io::sink()).expect("read entry");
        }
    }

    /// report raw vs compressed sizes for the real database
    #[test]
    fn export_stats() {
        use std::io::Read;
        let (db, _guard) = open_test_db();
        let mut buf = Vec::new();
        write_zip(&db, &mut buf).expect("write zip");

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&buf)).expect("parse zip");
        let mut raw = 0u64;
        let mut compressed = 0u64;
        let mut text_raw = 0u64;
        let mut text_compressed = 0u64;
        for i in 0..archive.len() {
            let file = archive.by_index(i).expect("open entry");
            raw += file.size();
            compressed += file.compressed_size();
            if !file.name().contains("/file/") {
                text_raw += file.size();
                text_compressed += file.compressed_size();
            }
        }
        let text_ratio = 100.0 * text_compressed as f64 / text_raw as f64;
        let files_raw = raw - text_raw;
        let files_compressed = compressed - text_compressed;
        let files_ratio = 100.0 * files_compressed as f64 / files_raw as f64;
        let total_ratio = 100.0 * compressed as f64 / raw as f64;
        println!("text: raw {text_raw} -> {text_compressed} ({text_ratio:.1}%)");
        println!("files: raw {files_raw} -> {files_compressed} ({files_ratio:.1}%)");
        println!("total: {total_ratio:.1}%");

        println!("--- root README.md ---");
        let mut root = archive.by_name("README.md").expect("open root readme");
        let mut index = String::new();
        root.read_to_string(&mut index).expect("read root readme");
        drop(root);
        println!("{index}");

        println!("--- archive tree ---");
        let mut names: Vec<String> = archive.file_names().map(String::from).collect();
        names.sort();
        for name in &names {
            let indent = "  ".repeat(name.matches('/').count());
            let leaf = name.rsplit('/').next().unwrap();
            println!("{indent}{leaf}");
        }
    }
}
