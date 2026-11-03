use rusqlite::Connection;
fn main() {
    let db = std::env::args().nth(1).unwrap_or("/data/app.db".into());
    let sql = std::env::args().nth(2).unwrap_or("select sqlite_version()".into());
    let c = Connection::open(&db).expect("open");
    let mut st = c.prepare(&sql).expect("prepare");
    let n = st.column_count();
    let rows = st.query_map([], |r| (0..n).map(|i| r.get::<_, rusqlite::types::Value>(i).map(|v| format!("{v:?}"))).collect::<Result<Vec<_>,_>>()).expect("query");
    for r in rows { println!("{}", r.unwrap().join(" | ")); }
}
