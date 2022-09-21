use crate::framework::prelude::*;

#[self::test(modules = Deno, start_chiseld = false, db = Sqlite)]
async fn from_0_6(mut c: TestContext) {
    c.chisel.write_bytes(".chiseld.db", include_bytes!("migrate_db/from_0_6/chiseld.db"));
    c.start_chiseld().await;

    c.chiseld.stderr
        .read_with_timeout("Migrated database from version \"0\"", Duration::from_secs(3)).await;
    c.chiseld.stderr.read("server is ready").await;
}

#[self::test(modules = Deno, start_chiseld = false, db = LegacySplitSqlite)]
async fn from_split(mut c: TestContext) {
    c.chisel.write_bytes("chiseld-meta.db", include_bytes!("migrate_db/from_split/chiseld-meta.db"));
    c.chisel.write_bytes("chiseld-data.db", include_bytes!("migrate_db/from_split/chiseld-data.db"));
    c.start_chiseld().await;

    c.chiseld.stderr
        .read_with_timeout("Migrated database from version \"0.7\"", Duration::from_secs(3)).await;
    c.chiseld.stderr.read("server is ready").await;
}

#[self::test(modules = Deno, start_chiseld = false, db = Sqlite)]
async fn from_0_12(mut c: TestContext) {
    c.chisel.write_bytes(".chiseld.db", include_bytes!("migrate_db/from_0_12/chiseld.db"));
    c.chisel.write_bytes(".chiseld.db-wal", include_bytes!("migrate_db/from_0_12/chiseld.db-wal"));
    c.start_chiseld().await;

    c.chisel.get("/node1/hello").send().await
        .assert_json(json!(["Hello Alice", "Hello Beth", "Hello Cynthia"]));
    c.chisel.get("/node2/hello").send().await
        .assert_json(json!(["Hi Adam", "Hi Bob", "Hi Cecil"]));
    c.chisel.get("/deno1/hello").send().await
        .assert_json(json!(["Hello Edmund", "Hello Henry", "Hello James"]));
}
