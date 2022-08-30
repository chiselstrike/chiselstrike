use crate::framework::prelude::*;

#[chisel_macros::test(modules = Node)]
pub async fn next_auth_crud(mut c: TestContext) {
    c.chisel
        .write(".env", r##"{ "CHISELD_AUTH_SECRET" : "1234" }"##);

    c.restart_chiseld().await;

    {
        // Create a new auth user
        c.chisel
            .post("/__chiselstrike/auth/users")
            .json(json!({"name":"Foo", "email":"foo@t.co"}))
            .header("ChiselAuth", "1234")
            .send()
            .await
            .assert_ok();

        // Delete the user
        c.chisel
            .delete("/__chiselstrike/auth/users?.name=Foo")
            .header("ChiselAuth", "1234")
            .send()
            .await
            .assert_ok();

        // Check it's not there anymore
        c.chisel
            .get("/__chiselstrike/auth/users")
            .header("ChiselAuth", "1234")
            .send()
            .await
            .assert_json(json!({"results": []}));
    }

    c.chisel
        .post("/__chiselstrike/auth/sessions")
        .json(json!({"sessionToken":"tok1", "userId":"id1", "expires":"2025-12-31"}))
        .header("ChiselAuth", "1234")
        .send()
        .await
        .assert_ok();

    c.chisel
        .post("/__chiselstrike/auth/tokens")
        .json(json!({"token":"tok1", "identifier":"id1", "expires":"2026-12-31"}))
        .header("ChiselAuth", "1234")
        .send()
        .await
        .assert_ok();

    c.chisel
        .post("/__chiselstrike/auth/accounts")
        .json(json!({
            "providerAccountId":"acct1",
            "userId":"usr1",
            "provider":"gh",
            "type":"oauth",
            "expires_at":42,
            "session_state":"good"
        }))
        .header("ChiselAuth", "1234")
        .send()
        .await
        .assert_ok();
}
