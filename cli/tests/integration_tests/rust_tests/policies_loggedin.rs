use crate::framework::prelude::*;

static TEST_ROUTE: &str = r##"
    export default function() {
        return "ok";
    }
    "##;

async fn store_user(chisel: &Chisel, name: &str, email: &str) -> String {
    let user_json = chisel
        .post("/__chiselstrike/auth/users")
        .header("ChiselAuth", "dud")
        .json(json!({"name": name, "email": email}))
        .send()
        .await
        .json();

    user_json["id"].as_str().unwrap().into()
}

#[self::test(modules = Deno, optimize = Yes)]
async fn basic(mut c: TestContext) {
    c.chisel.write_unindent("routes/test.ts", TEST_ROUTE);
    c.chisel
        .write_unindent("routes/a/b/c/testc1.ts", TEST_ROUTE);
    c.chisel
        .write_unindent("routes/a/b/c/testc2.ts", TEST_ROUTE);
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
            routes:
            - path: /test
              users: .*
            - path: /a/b/c/testc1
              users: ^al$
            - path: /a/b
              users: ^a
        "##,
    );
    c.chisel
        .write(".env", r#"{ "CHISELD_AUTH_SECRET": "dud" }"#);
    c.chisel.apply_ok().await;

    let id_al = store_user(&c.chisel, "Al", "al").await;
    let id_als = store_user(&c.chisel, "Als", "als").await;

    c.chisel.get("/dev/test").send().await.assert_status(403);
    c.restart_chiseld().await;

    c.chisel.get("/dev/test").send().await.assert_status(403);
    c.chisel
        .get("/dev/test")
        .header("ChiselUID", &id_al)
        .send()
        .await
        .assert_status(200);
    c.chisel
        .get("/dev/test")
        .header("ChiselUID", "invalid_id")
        .send()
        .await
        .assert_status(403);

    c.chisel
        .get("/dev/a/b/c/testc1")
        .send()
        .await
        .assert_status(403);
    c.chisel
        .get("/dev/a/b/c/testc1")
        .header("ChiselUID", "invalid_id")
        .send()
        .await
        .assert_status(403);
    c.chisel
        .get("/dev/a/b/c/testc1")
        .header("ChiselUID", &id_al)
        .send()
        .await
        .assert_status(200);
    c.chisel
        .get("/dev/a/b/c/testc1")
        .header("ChiselUID", &id_als)
        .send()
        .await
        .assert_status(403);

    c.chisel
        .get("/dev/a/b/c/testc2")
        .send()
        .await
        .assert_status(403);
    c.chisel
        .get("/dev/a/b/c/testc2")
        .header("ChiselUID", "invalid_id")
        .send()
        .await
        .assert_status(403);
    c.chisel
        .get("/dev/a/b/c/testc2")
        .header("ChiselUID", &id_al)
        .send()
        .await
        .assert_status(200);
    c.chisel
        .get("/dev/a/b/c/testc2")
        .header("ChiselUID", &id_als)
        .send()
        .await
        .assert_status(200);
}

#[self::test(modules = Deno, optimize = Yes)]
async fn endpoints_backcompat(c: TestContext) {
    // test that we support `endpoints:` instead of `routes:` for backwards compatibility
    c.chisel.write_unindent("routes/test.ts", TEST_ROUTE);
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
            endpoints:
            - path: /test
              users: ^$
        "##,
    );
    c.chisel.apply_ok().await;

    c.chisel.get("/dev/test").send().await.assert_status(403);
}

#[self::test(modules = Deno, optimize = Yes)]
async fn repeated_path(c: TestContext) {
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
            routes:
            - path: /find
              users: .*
            - path: /find
              users: ^als$
        "##,
    );
    c.chisel
        .apply_err()
        .await
        .stderr
        .read("Error: Repeated path in user authorization: \"/find\"");
}

static MODEL_POST: &str = r##"
    import { ChiselEntity, AuthUser, labels } from '@chiselstrike/api'
    export class Post extends ChiselEntity {
        text: string = "Lorem Ipsum";
        @labels("protect") author: AuthUser;
    }
    export class Blog extends ChiselEntity {
        post1?: Post;
    }
"##;

static ROUTE_POSTS: &str = r##"
    import { Post } from '../models/post.ts';
    import { loggedInUser, responseFromJson } from '@chiselstrike/api';
    export default async function (req: Request) {
        if (req.method == 'POST') {
            let p = Post.build(await req.json());
            let author = await loggedInUser();
            if (author === undefined) return new Response('Must be logged in', {status: 401});
            p.author = author;
            await p.save();
            return new Response('saved post successfully');
        } else if (req.method == 'GET') {
            let r: Array<Pick<Post, "text">> = [];
            await Post.cursor().select('text').forEach(p => r.push(p));
            return r;
        }
    }
"##;

async fn store_post(chisel: &Chisel, uid: &str, text: &str) {
    chisel
        .post("/dev/posts")
        .header("ChiselUID", uid)
        .json(json!({ "text": text }))
        .send()
        .await
        .assert_ok();
}

#[self::test(modules = Deno, optimize = Both)]
async fn logged_in_user_post(c: TestContext) {
    c.chisel.write_unindent("models/post.ts", MODEL_POST);
    c.chisel.write_unindent("routes/posts.ts", ROUTE_POSTS);
    c.chisel.apply_ok().await;

    let id_al = store_user(&c.chisel, "Al", "al").await;
    let id_als = store_user(&c.chisel, "Als", "als").await;

    store_post(&c.chisel, &id_al, "first post by al").await;
    store_post(&c.chisel, &id_als, "first post by als").await;
    store_post(&c.chisel, &id_al, "second post by al").await;

    c.chisel
        .get("/dev/posts")
        .header("ChiselUID", &id_al)
        .send()
        .await
        .assert_json(json!([
            {"text": "first post by al"},
            {"text": "first post by als"},
            {"text": "second post by al"},
        ]));
}

#[self::test(modules = Deno, optimize = Both)]
async fn transform_match_login(c: TestContext) {
    c.chisel.write_unindent("models/post.ts", MODEL_POST);
    c.chisel.write_unindent("routes/posts.ts", ROUTE_POSTS);
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
            labels:
            - name: protect
              transform: match_login
        "##,
    );
    c.chisel.apply_ok().await;

    let id_al = store_user(&c.chisel, "Al", "al").await;
    let id_als = store_user(&c.chisel, "Als", "als").await;

    store_post(&c.chisel, &id_al, "first post by al").await;
    store_post(&c.chisel, &id_als, "first post by als").await;
    store_post(&c.chisel, &id_al, "second post by al").await;

    let resp = c
        .chisel
        .get("/dev/posts")
        .header("ChiselUID", &id_al)
        .send()
        .await
        .assert_ok()
        .json();
    assert_eq!(resp.as_array().unwrap().len(), 2);
}

static ROUTE_BLOGS: &str = r##"
    import { Blog, Post } from '../models/post.ts';
    import { loggedInUser, responseFromJson } from '@chiselstrike/api';
    export default async function chisel(req: Request) {
        if (req.method == 'POST') {
            const b = Blog.build(await req.json());
            const author = await loggedInUser();
            if (author === undefined) return new Response('Must be logged in', {status: 401});
            b.post1!.author = author;
            await b.save();
            return new Response('saved post successfully');
        } else if (req.method == 'GET') {
            const r = (await Blog.cursor().toArray()).map(b => b.post1!.text);
            return responseFromJson(r);
        }
    }
"##;

async fn store_blog_post(chisel: &Chisel, uid: &str, text: &str) {
    chisel
        .post("/dev/blogs")
        .header("ChiselUID", uid)
        .json(json!({"post1": {"text": text}}))
        .send()
        .await
        .assert_status(200)
        .assert_text("saved post successfully");
}

#[self::test(modules = Deno, optimize = Both)]
async fn transform_match_login_related_entities(c: TestContext) {
    c.chisel.write_unindent("models/post.ts", MODEL_POST);
    c.chisel.write_unindent("routes/posts.ts", ROUTE_POSTS);
    c.chisel.write_unindent("routes/blogs.ts", ROUTE_BLOGS);
    c.chisel.apply_ok().await;

    let id_al = store_user(&c.chisel, "Al", "al").await;
    let id_als = store_user(&c.chisel, "Als", "als").await;

    c.chisel
        .post("/dev/posts")
        .json(json!({"text": "first post by al"}))
        .send()
        .await
        .assert_status(401)
        .assert_text("Must be logged in");

    c.chisel
        .post("/dev/blogs")
        .json(json!({"post1": {"text": "first post by al"}}))
        .send()
        .await
        .assert_status(401)
        .assert_text("Must be logged in");

    store_blog_post(&c.chisel, &id_al, "first blog post by al").await;
    store_blog_post(&c.chisel, &id_als, "first blog post by als").await;
    store_blog_post(&c.chisel, &id_al, "second blog post by al").await;

    c.chisel
        .get("/dev/blogs")
        .header("ChiselUID", &id_al)
        .send()
        .await
        .assert_status(200)
        .assert_json(json!([
            "first blog post by al",
            "first blog post by als",
            "second blog post by al",
        ]));

    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
            labels:
            - name: protect
              transform: match_login
        "##,
    );
    c.chisel.apply_ok().await;

    c.chisel
        .get("/dev/blogs")
        .header("ChiselUID", &id_al)
        .send()
        .await
        .assert_status(200)
        .assert_json(json!(["first blog post by al", "second blog post by al",]));
}
