//! sqlite: SQL over the database files under /data.
//!
//! SQLite's famous escapes are its `.shell` and `.system` dot-commands,
//! `ATTACH` to any path, and `load_extension`. Here there is no process to
//! start, so `.shell` cannot exist; the only files are in the folder the
//! manifest mounts for this component; and extensions stay off. The boundary
//! is the manifest, not this code.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/sqlite-tool",
    generate_all,
});

use exports::workshop::toolbox::sqlite::{Error, Guest, QueryArgs};
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::{Connection, OpenFlags, types::ValueRef};

/// The database this tool opens, unless the manifest's environment names
/// another with SQLITE_DB.
const DEFAULT_DB: &str = "/data/main.db";

fn db_path() -> String {
    std::env::var("SQLITE_DB").unwrap_or_else(|_| DEFAULT_DB.to_string())
}

/// SQLite's error for `what`, as this tool's typed error.
fn sql_error(what: &str, e: rusqlite::Error) -> Error {
    use rusqlite::ErrorCode as C;
    match &e {
        rusqlite::Error::SqliteFailure(f, _) => match f.code {
            C::CannotOpen => Error::AccessDenied(format!("{what}: {e}")),
            C::ReadOnly | C::PermissionDenied | C::AuthorizationForStatementDenied => {
                Error::AccessDenied(format!("{what}: {e}"))
            }
            _ => Error::InvalidArgument(e.to_string()),
        },
        _ => Error::InvalidArgument(e.to_string()),
    }
}

fn open() -> Result<Connection, Error> {
    let path = db_path();
    if !std::path::Path::new(&path).exists() {
        return Err(Error::NotFound(format!("{path}: No such file or directory")));
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(&path, flags).map_err(|e| sql_error(&path, e))?;
    // A read-only mount can't take SQLite's locks, so even reads fail there.
    // Reopen it as an immutable file: no locks, no journal, reads only.
    match conn.query_row("select count(*) from sqlite_schema", [], |_| Ok(())) {
        Ok(()) => Ok(conn),
        Err(_) => Connection::open_with_flags(
            format!("file:{path}?immutable=1"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(|e| sql_error(&path, e)),
    }
}

/// Run one statement and print its rows the way sqlite3's list mode does.
fn rows(conn: &Connection, sql: &str) -> Result<String, Error> {
    let mut out = String::new();
    let mut batch = rusqlite::Batch::new(conn, sql);
    while let Some(mut stmt) = batch.next().map_err(|e| sql_error(sql, e))? {
        let n = stmt.column_count();
        let mut q = stmt.raw_query();
        while let Some(row) = q.next().map_err(|e| sql_error(sql, e))? {
            let cols: Vec<String> = (0..n)
                .map(|i| match row.get_ref(i) {
                    Ok(ValueRef::Null) | Err(_) => String::new(),
                    Ok(ValueRef::Integer(v)) => v.to_string(),
                    Ok(ValueRef::Real(v)) => v.to_string(),
                    Ok(ValueRef::Text(t)) => String::from_utf8_lossy(t).into_owned(),
                    Ok(ValueRef::Blob(b)) => format!("<{} bytes>", b.len()),
                })
                .collect();
            out.push_str(&cols.join(" | "));
            out.push('\n');
        }
    }
    Ok(out)
}

/// The dot-commands this tool knows, as SQL.
fn dot_command(line: &str) -> Result<String, Error> {
    let mut words = line.split_whitespace();
    let cmd = words.next().unwrap_or_default();
    let arg = words.next();
    match (cmd, arg) {
        (".tables", _) => Ok("select name from sqlite_schema where type in ('table','view') \
                              and name not like 'sqlite_%' order by name"
            .into()),
        (".schema", None) => Ok("select sql from sqlite_schema where sql is not null".into()),
        (".schema", Some(t)) => Ok(format!(
            "select sql from sqlite_schema where tbl_name = '{}'",
            t.replace('\'', "''")
        )),
        (".shell" | ".system", _) => Err(Error::InvalidArgument(format!(
            "{cmd}: there is no shell here. This sqlite3 runs as a component that can't \
             start processes."
        ))),
        _ => Err(Error::InvalidArgument(format!("unknown command: {cmd}"))),
    }
}

struct Sqlite;

impl Guest for Sqlite {
    async fn query(args: QueryArgs) -> Result<String, Error> {
        let sql = args.sql.trim();
        if sql.is_empty() {
            return Err(Error::InvalidArgument("sql is empty".into()));
        }
        let conn = open()?;
        let sql = if sql.starts_with('.') { dot_command(sql)? } else { sql.to_string() };
        rows(&conn, &sql)
    }
}

export!(Sqlite);
