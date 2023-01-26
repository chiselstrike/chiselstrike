use serde::Deserialize;

use crate::framework::prelude::{test, *};

#[test(modules = Both)]
async fn basic(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/person', () => "GET index.ts /person");
        "#,
    );

    c.chisel.write(
        "routes/user.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/', () => "GET user.ts /")
            .get('/alice', () => "GET user.ts /alice");
        "#,
    );

    c.chisel.write(
        "routes/user/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/bob', () => "GET user/index.ts /bob");
        "#,
    );

    c.chisel.write(
        "routes/user/helen.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/', () => "GET user/helen.ts /");
        "#,
    );

    c.chisel.apply_ok().await;

    c.chisel
        .get("/dev/person")
        .send()
        .await
        .assert_text("GET index.ts /person");
    c.chisel
        .get("/dev/user")
        .send()
        .await
        .assert_text("GET user.ts /");
    c.chisel
        .get("/dev/user/alice")
        .send()
        .await
        .assert_text("GET user.ts /alice");
    c.chisel
        .get("/dev/user/bob")
        .send()
        .await
        .assert_text("GET user/index.ts /bob");
    c.chisel
        .get("/dev/user/helen")
        .send()
        .await
        .assert_text("GET user/helen.ts /");
}

#[test(modules = Deno)]
async fn params_in_code(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/route1/:x',
                (req) => [1, req.params.get('x')])
            .get('/route2/:x/:y',
                (req) => [2, req.params.get('x'), req.params.get('y')])
            .get('/:x/route3/:y',
                (req) => [3, req.params.get('x'), req.params.get('y')]);
        "#,
    );

    c.chisel.apply_ok().await;

    c.chisel
        .get("/dev/route1/xyz")
        .send()
        .await
        .assert_json(json!([1, "xyz"]));
    c.chisel
        .get("/dev/route1/!@$^")
        .send()
        .await
        .assert_json(json!([1, "!@$^"]));
    c.chisel.get("/dev/route1/").send().await.assert_status(404);
    c.chisel
        .get("/dev/route1/xyz/abc")
        .send()
        .await
        .assert_status(404);

    c.chisel
        .get("/dev/route2/abc/xyz")
        .send()
        .await
        .assert_json(json!([2, "abc", "xyz"]));
    c.chisel
        .get("/dev/abc/route3/xyz")
        .send()
        .await
        .assert_json(json!([3, "abc", "xyz"]));
}

#[test(modules = Both)]
async fn params_in_files(c: TestContext) {
    c.chisel.write(
        "routes/route1/[x].ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('', (req) => [1, req.params.get('x')]);
        "#,
    );

    c.chisel.write(
        "routes/route2/[x]/[y].ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('', (req) => [2, req.params.get('x'), req.params.get('y')]);
        "#,
    );

    c.chisel.write(
        "routes/[x]/route3/[y].ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('', (req) => [3, req.params.get('x'), req.params.get('y')]);
        "#,
    );

    c.chisel.apply_ok().await;

    c.chisel
        .get("/dev/route1/xyz")
        .send()
        .await
        .assert_json(json!([1, "xyz"]));
    c.chisel
        .get("/dev/route2/abc/xyz")
        .send()
        .await
        .assert_json(json!([2, "abc", "xyz"]));
    c.chisel
        .get("/dev/abc/route3/xyz")
        .send()
        .await
        .assert_json(json!([3, "abc", "xyz"]));
}

#[test(modules = Deno)]
async fn params_get_typed(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/string/:x', (req) => ['string', req.params.get('x')])
            .get('/number/:x', (req) => ['number', req.params.getNumber('x')])
            .get('/int/:x', (req) => ['int', req.params.getInt('x')])
            .get('/bool/:x', (req) => ['bool', req.params.getBool('x')]);
        "#,
    );

    c.chisel.apply_ok().await;

    c.chisel
        .get("/dev/string/abc")
        .send()
        .await
        .assert_json(json!(["string", "abc"]));
    c.chisel
        .get("/dev/string/null")
        .send()
        .await
        .assert_json(json!(["string", "null"]));
    c.chisel
        .get("/dev/string/123")
        .send()
        .await
        .assert_json(json!(["string", "123"]));

    c.chisel
        .get("/dev/number/10")
        .send()
        .await
        .assert_json(json!(["number", 10]));
    c.chisel
        .get("/dev/number/0.125")
        .send()
        .await
        .assert_json(json!(["number", 0.125]));
    c.chisel
        .get("/dev/number/-10.5")
        .send()
        .await
        .assert_json(json!(["number", -10.5]));
    c.chisel
        .get("/dev/number/3e2")
        .send()
        .await
        .assert_json(json!(["number", 300]));
    c.chisel
        .get("/dev/number/0.5foo")
        .send()
        .await
        .assert_json(json!(["number", 0.5]));
    c.chisel
        .get("/dev/number/foo")
        .send()
        .await
        .assert_json(json!(["number", null]));
    c.chisel
        .get("/dev/number/infinity")
        .send()
        .await
        .assert_json(json!(["number", null]));

    c.chisel
        .get("/dev/int/10")
        .send()
        .await
        .assert_json(json!(["int", 10]));
    c.chisel
        .get("/dev/int/1234567")
        .send()
        .await
        .assert_json(json!(["int", 1234567]));
    c.chisel
        .get("/dev/int/-42")
        .send()
        .await
        .assert_json(json!(["int", -42]));
    c.chisel
        .get("/dev/int/+42")
        .send()
        .await
        .assert_json(json!(["int", 42]));
    c.chisel
        .get("/dev/int/010")
        .send()
        .await
        .assert_json(json!(["int", 10]));
    c.chisel
        .get("/dev/int/0x10")
        .send()
        .await
        .assert_json(json!(["int", 0]));
    c.chisel
        .get("/dev/int/foo")
        .send()
        .await
        .assert_json(json!(["int", null]));
    c.chisel
        .get("/dev/int/42foo")
        .send()
        .await
        .assert_json(json!(["int", 42]));
    c.chisel
        .get("/dev/int/42.send().5")
        .send()
        .await
        .assert_json(json!(["int", 42]));
    c.chisel
        .get("/dev/int/3e2")
        .send()
        .await
        .assert_json(json!(["int", 3]));

    c.chisel
        .get("/dev/bool/true")
        .send()
        .await
        .assert_json(json!(["bool", true]));
    c.chisel
        .get("/dev/bool/1")
        .send()
        .await
        .assert_json(json!(["bool", true]));
    c.chisel
        .get("/dev/bool/false")
        .send()
        .await
        .assert_json(json!(["bool", false]));
    c.chisel
        .get("/dev/bool/0")
        .send()
        .await
        .assert_json(json!(["bool", false]));
    c.chisel
        .get("/dev/bool/something")
        .send()
        .await
        .assert_json(json!(["bool", true]));
}

#[test(modules = Deno)]
async fn params_get_wrong(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/missing/string', (req) => req.params.get('x'))
            .get('/missing/number', (req) => req.params.getNumber('x'))
            .get('/missing/int', (req) => req.params.getInt('x'))
            .get('/missing/bool', (req) => req.params.getBool('x'))
            .middleware(async function (req, next) {
                try {
                    return await next(req);
                } catch (e) {
                    return new Response(e + '', {status: 500});
                }
            });
        "#,
    );

    c.chisel.apply_ok().await;

    c.chisel
        .get("/dev/missing/string")
        .send()
        .await
        .assert_text_contains("Error: Undefined parameter 'x'");
    c.chisel
        .get("/dev/missing/number")
        .send()
        .await
        .assert_text_contains("Error: Undefined parameter 'x'");
    c.chisel
        .get("/dev/missing/int")
        .send()
        .await
        .assert_text_contains("Error: Undefined parameter 'x'");
    c.chisel
        .get("/dev/missing/bool")
        .send()
        .await
        .assert_text_contains("Error: Undefined parameter 'x'");
}

#[test(modules = Deno)]
async fn slashes(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .prefix('/route1', new RouteMap()
                .get('/', () => [1, "/"]))
            .prefix('/route2/', new RouteMap()
                .get('/', () => [2, "/"]))
            .prefix('route3/', new RouteMap()
                .get('/', () => [3, "/"]))
            .prefix('route4', new RouteMap()
                .get('/', () => [4, "/"]))

            .prefix('/route5', new RouteMap()
                .get('', () => [5, ""]))
            .prefix('/route6', new RouteMap()
                .get('', () => [6, ""])
                .get('/', () => [6, "/"]))
            .prefix('/route7', new RouteMap()
                .get('/', () => [7, "/"])
                .get('', () => [7, ""]))
        "#,
    );

    c.chisel.apply_ok().await;

    assert_eq!(c.chisel.get_json("/dev/route1").await, json!([1, "/"]));
    assert_eq!(c.chisel.get_json("/dev/route1/").await, json!([1, "/"]));
    assert_eq!(c.chisel.get_json("/dev/route2").await, json!([2, "/"]));
    assert_eq!(c.chisel.get_json("/dev/route2/").await, json!([2, "/"]));
    assert_eq!(c.chisel.get_json("/dev/route3").await, json!([3, "/"]));
    assert_eq!(c.chisel.get_json("/dev/route3/").await, json!([3, "/"]));
    assert_eq!(c.chisel.get_json("/dev/route4").await, json!([4, "/"]));
    assert_eq!(c.chisel.get_json("/dev/route4/").await, json!([4, "/"]));

    assert_eq!(c.chisel.get_json("/dev/route5").await, json!([5, ""]));
    assert_eq!(c.chisel.get_json("/dev/route5/").await, json!([5, ""]));
    assert_eq!(c.chisel.get_json("/dev/route6").await, json!([6, ""]));
    assert_eq!(c.chisel.get_json("/dev/route6/").await, json!([6, ""]));
    assert_eq!(c.chisel.get_json("/dev/route7").await, json!([7, "/"]));
    assert_eq!(c.chisel.get_json("/dev/route7/").await, json!([7, "/"]));
}

#[test(modules = Deno)]
async fn method_shorthands(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/route', () => ["GET"])
            .post('/route', () => ["POST"])
            .put('/route', () => ["PUT"])
            .delete('/route', () => ["DELETE"])
            .patch('/route', () => ["PATCH"]);
        "#,
    );

    c.chisel.apply_ok().await;

    c.chisel
        .request(Method::GET, "/dev/route")
        .send()
        .await
        .assert_json(json!(["GET"]));
    c.chisel
        .request(Method::POST, "/dev/route")
        .send()
        .await
        .assert_json(json!(["POST"]));
    c.chisel
        .request(Method::PUT, "/dev/route")
        .send()
        .await
        .assert_json(json!(["PUT"]));
    c.chisel
        .request(Method::DELETE, "/dev/route")
        .send()
        .await
        .assert_json(json!(["DELETE"]));
    c.chisel
        .request(Method::PATCH, "/dev/route")
        .send()
        .await
        .assert_json(json!(["PATCH"]));
}

#[test(modules = Deno)]
async fn method_manual(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .route('get', '/route1', () => [1, "get"])
            .route('post', '/route1', () => [1, "post"])

            .route(['get', 'post'], '/route2', () => [2])

            .route('DELETE', '/route3', () => [3, "delete"])
            .route('patch', '/route3', () => [3, "patch"])

            .route('*', '/route4', () => [4]);
        "#,
    );

    c.chisel.apply_ok().await;

    c.chisel
        .request(Method::GET, "/dev/route1")
        .send()
        .await
        .assert_json(json!([1, "get"]));
    c.chisel
        .request(Method::POST, "/dev/route1")
        .send()
        .await
        .assert_json(json!([1, "post"]));
    c.chisel
        .request(Method::DELETE, "/dev/route1")
        .send()
        .await
        .assert_status(405);

    c.chisel
        .request(Method::GET, "/dev/route2")
        .send()
        .await
        .assert_json(json!([2]));
    c.chisel
        .request(Method::POST, "/dev/route2")
        .send()
        .await
        .assert_json(json!([2]));
    c.chisel
        .request(Method::DELETE, "/dev/route2")
        .send()
        .await
        .assert_status(405);

    c.chisel
        .request(Method::DELETE, "/dev/route3")
        .send()
        .await
        .assert_json(json!([3, "delete"]));
    c.chisel
        .request(Method::PATCH, "/dev/route3")
        .send()
        .await
        .assert_json(json!([3, "patch"]));
    c.chisel
        .request(Method::GET, "/dev/route3")
        .send()
        .await
        .assert_status(405);

    c.chisel
        .request(Method::GET, "/dev/route4")
        .send()
        .await
        .assert_json(json!([4]));
    c.chisel
        .request(Method::POST, "/dev/route4")
        .send()
        .await
        .assert_json(json!([4]));
    c.chisel
        .request(Method::PATCH, "/dev/route4")
        .send()
        .await
        .assert_json(json!([4]));
}

#[test(modules = Deno)]
async fn errors(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/route1', function () { throw new Error("it blew up") })
            // @ts-expect-error
            .get('/route2', () => nonexistent)
            .post('/route3', () => [3]);
        "#,
    );

    c.chisel.apply_ok().await;

    c.chisel
        .get("/dev/route1")
        .send()
        .await
        .assert_status(500)
        .assert_text_contains("Error in GET /dev/route1: Error: it blew up");

    c.chisel
        .get("/dev/route2")
        .send()
        .await
        .assert_status(500)
        .assert_text_contains(
            "Error in GET /dev/route2: ReferenceError: nonexistent is not defined",
        );

    c.chisel
        .get("/dev/route3")
        .send()
        .await
        .assert_status(405)
        .assert_text("Method GET is not supported for \"/route3\"");

    c.chisel
        .get("/dev/nonexistent")
        .send()
        .await
        .assert_status(404)
        .assert_text("There is no route for \"/nonexistent\"");
}

#[test(modules = Deno)]
async fn route_reflection(c: TestContext) {
    c.chisel.write(
        "models/person.ts",
        r#"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Person extends ChiselEntity {
            first_name: string = "";
            last_name: string = "";
            age: number = 0;
            human: boolean = false;
            height: number = 1;
        }
    "#,
    );
    c.chisel.write(
        "routes/people.ts",
        r#"
        import { Person } from "../models/person.ts";
        export default Person.crud();
    "#,
    );
    c.chisel.apply_ok().await;

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CrudHandler {
        entity_name: String,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(tag = "kind", content = "handler", rename_all = "camelCase")]
    enum HandlerKind {
        Crud(CrudHandler),
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CrudMeta {
        handler: HandlerKind,
    }
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Route {
        methods: Vec<String>,
        path_pattern: String,
        client_metadata: Option<CrudMeta>,
    }

    let routes_json = c.chisel.get_json("/dev/__chiselstrike/routes").await;
    let routes: Vec<Route> = serde_json::from_value(routes_json).unwrap();
    for r in routes {
        if r.path_pattern == "/people/" && r.methods == vec!["GET"] {
            let HandlerKind::Crud(handler) = r.client_metadata.unwrap().handler;
            assert_eq!(handler.entity_name, "Person");
        }
    }
}
