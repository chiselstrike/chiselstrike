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

#[chisel_macros::test(modules = Node)]
pub async fn cant_save_auth_from_user_route(mut c: TestContext) {
    c.chisel
        .write(".env", r##"{ "CHISELD_AUTH_SECRET" : "1234" }"##);
    c.chisel.write(
        "routes/auth_users.ts",
        r#"
        import { AuthUser } from '@chiselstrike/api';
        export default AuthUser.crud();"#,
    );
    c.chisel.apply_ok().await;
    c.restart_chiseld().await;

    c.chisel
        .post("/dev/auth_users")
        .json(json!({"name":"Foo", "email":"foo@t.co"}))
        .send()
        .await
        .assert_status(500)
        .assert_text_contains("Error: Cannot save into auth type AuthUser");
    c.chisel
        .put("/dev/auth_users/abcd-1234-eeee-5678")
        .json(json!({"name":"Foo", "email":"foo@t.co"}))
        .send()
        .await
        .assert_status(500)
        .assert_text_contains("Error: Cannot save into auth type AuthUser");

    // Verify that no user was saved.
    c.chisel
        .get("/__chiselstrike/auth/users")
        .header("ChiselAuth", "1234")
        .send()
        .await
        .assert_json(json!({"results": []}));
}

#[chisel_macros::test(modules = Node)]
pub async fn cant_save_auth_from_user_route_via_relation(mut c: TestContext) {
    c.chisel
        .write(".env", r##"{ "CHISELD_AUTH_SECRET" : "1234" }"##);
    c.chisel.write(
        "models/model.ts",
        r#"
        import { ChiselEntity, AuthUser } from '@chiselstrike/api'
        export class SomeModel extends ChiselEntity {
            text: string = "Lorem Ipsum";
            author: AuthUser;
        }"#,
    );
    c.chisel.write(
        "routes/some_models.ts",
        r#"
        import { SomeModel } from '../models/model.ts';
        export default SomeModel.crud();"#,
    );
    c.chisel.apply_ok().await;
    c.restart_chiseld().await;

    c.chisel
        .post("/dev/some_models")
        .json(json!({"author":{"email": "foo@t.co"}}))
        .send()
        .await
        .assert_status(500)
        .assert_text_contains("Error: Cannot save into nested type AuthUser");
    c.chisel
        .post_json(
            "/dev/some_models",
            json!({"author":{"id":"ID123", "email": "foo@t.co"}}),
        )
        .await;

    // Verify that no user was saved.
    c.chisel
        .get("/__chiselstrike/auth/users")
        .header("ChiselAuth", "1234")
        .send()
        .await
        .assert_json(json!({"results": []}));
}
