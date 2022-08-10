use crate::framework::prelude::{*, test};

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

    assert_eq!(
        c.chisel.get_text("/dev/person").await,
        "GET index.ts /person"
    );
    assert_eq!(c.chisel.get_text("/dev/user").await, "GET user.ts /");
    assert_eq!(
        c.chisel.get_text("/dev/user/alice").await,
        "GET user.ts /alice"
    );
    assert_eq!(
        c.chisel.get_text("/dev/user/bob").await,
        "GET user/index.ts /bob"
    );
    assert_eq!(
        c.chisel.get_text("/dev/user/helen").await,
        "GET user/helen.ts /"
    );
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

    assert_eq!(
        c.chisel.get_json("/dev/route1/xyz").await,
        json!([1, "xyz"])
    );
    assert_eq!(
        c.chisel.get_json("/dev/route1/!@$^").await,
        json!([1, "!@$^"])
    );
    assert_eq!(c.chisel.get_status("/dev/route1/").await, 404);
    assert_eq!(c.chisel.get_status("/dev/route1/xyz/abc").await, 404);

    assert_eq!(
        c.chisel.get_json("/dev/route2/abc/xyz").await,
        json!([2, "abc", "xyz"])
    );
    assert_eq!(
        c.chisel.get_json("/dev/abc/route3/xyz").await,
        json!([3, "abc", "xyz"])
    );
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

    assert_eq!(
        c.chisel.get_json("/dev/route1/xyz").await,
        json!([1, "xyz"])
    );
    assert_eq!(
        c.chisel.get_json("/dev/route2/abc/xyz").await,
        json!([2, "abc", "xyz"])
    );
    assert_eq!(
        c.chisel.get_json("/dev/abc/route3/xyz").await,
        json!([3, "abc", "xyz"])
    );
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

    assert_eq!(
        c.chisel.get_json("/dev/string/abc").await,
        json!(["string", "abc"])
    );
    assert_eq!(
        c.chisel.get_json("/dev/string/null").await,
        json!(["string", "null"])
    );
    assert_eq!(
        c.chisel.get_json("/dev/string/123").await,
        json!(["string", "123"])
    );

    assert_eq!(
        c.chisel.get_json("/dev/number/10").await,
        json!(["number", 10])
    );
    assert_eq!(
        c.chisel.get_json("/dev/number/0.125").await,
        json!(["number", 0.125])
    );
    assert_eq!(
        c.chisel.get_json("/dev/number/-10.5").await,
        json!(["number", -10.5])
    );
    assert_eq!(
        c.chisel.get_json("/dev/number/3e2").await,
        json!(["number", 300])
    );
    assert_eq!(
        c.chisel.get_json("/dev/number/0.5foo").await,
        json!(["number", 0.5])
    );
    assert_eq!(
        c.chisel.get_json("/dev/number/foo").await,
        json!(["number", null])
    );
    assert_eq!(
        c.chisel.get_json("/dev/number/infinity").await,
        json!(["number", null])
    );

    assert_eq!(c.chisel.get_json("/dev/int/10").await, json!(["int", 10]));
    assert_eq!(
        c.chisel.get_json("/dev/int/1234567").await,
        json!(["int", 1234567])
    );
    assert_eq!(c.chisel.get_json("/dev/int/-42").await, json!(["int", -42]));
    assert_eq!(c.chisel.get_json("/dev/int/+42").await, json!(["int", 42]));
    assert_eq!(c.chisel.get_json("/dev/int/010").await, json!(["int", 10]));
    assert_eq!(c.chisel.get_json("/dev/int/0x10").await, json!(["int", 0]));
    assert_eq!(
        c.chisel.get_json("/dev/int/foo").await,
        json!(["int", null])
    );
    assert_eq!(
        c.chisel.get_json("/dev/int/42foo").await,
        json!(["int", 42])
    );
    assert_eq!(c.chisel.get_json("/dev/int/42.5").await, json!(["int", 42]));
    assert_eq!(c.chisel.get_json("/dev/int/3e2").await, json!(["int", 3]));

    assert_eq!(
        c.chisel.get_json("/dev/bool/true").await,
        json!(["bool", true])
    );
    assert_eq!(
        c.chisel.get_json("/dev/bool/1").await,
        json!(["bool", true])
    );
    assert_eq!(
        c.chisel.get_json("/dev/bool/false").await,
        json!(["bool", false])
    );
    assert_eq!(
        c.chisel.get_json("/dev/bool/0").await,
        json!(["bool", false])
    );
    assert_eq!(
        c.chisel.get_json("/dev/bool/something").await,
        json!(["bool", true])
    );
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

    assert_eq!(
        c.chisel.get_text("/dev/missing/string").await,
        "Error: Undefined parameter 'x'"
    );
    assert_eq!(
        c.chisel.get_text("/dev/missing/number").await,
        "Error: Undefined parameter 'x'"
    );
    assert_eq!(
        c.chisel.get_text("/dev/missing/int").await,
        "Error: Undefined parameter 'x'"
    );
    assert_eq!(
        c.chisel.get_text("/dev/missing/bool").await,
        "Error: Undefined parameter 'x'"
    );
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

    assert_eq!(
        c.chisel.request_json(Method::GET, "/dev/route", "").await,
        json!(["GET"])
    );
    assert_eq!(
        c.chisel.request_json(Method::POST, "/dev/route", "").await,
        json!(["POST"])
    );
    assert_eq!(
        c.chisel.request_json(Method::PUT, "/dev/route", "").await,
        json!(["PUT"])
    );
    assert_eq!(
        c.chisel
            .request_json(Method::DELETE, "/dev/route", "")
            .await,
        json!(["DELETE"])
    );
    assert_eq!(
        c.chisel.request_json(Method::PATCH, "/dev/route", "").await,
        json!(["PATCH"])
    );
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

    assert_eq!(
        c.chisel.request_json(Method::GET, "/dev/route1", "").await,
        json!([1, "get"])
    );
    assert_eq!(
        c.chisel.request_json(Method::POST, "/dev/route1", "").await,
        json!([1, "post"])
    );
    assert_eq!(
        c.chisel
            .request_status(Method::DELETE, "/dev/route1", "")
            .await,
        405
    );

    assert_eq!(
        c.chisel.request_json(Method::GET, "/dev/route2", "").await,
        json!([2])
    );
    assert_eq!(
        c.chisel.request_json(Method::POST, "/dev/route2", "").await,
        json!([2])
    );
    assert_eq!(
        c.chisel
            .request_status(Method::DELETE, "/dev/route2", "")
            .await,
        405
    );

    assert_eq!(
        c.chisel
            .request_json(Method::DELETE, "/dev/route3", "")
            .await,
        json!([3, "delete"])
    );
    assert_eq!(
        c.chisel
            .request_json(Method::PATCH, "/dev/route3", "")
            .await,
        json!([3, "patch"])
    );
    assert_eq!(
        c.chisel
            .request_status(Method::GET, "/dev/route3", "")
            .await,
        405
    );

    assert_eq!(
        c.chisel.request_json(Method::GET, "/dev/route4", "").await,
        json!([4])
    );
    assert_eq!(
        c.chisel.request_json(Method::POST, "/dev/route4", "").await,
        json!([4])
    );
    assert_eq!(
        c.chisel
            .request_json(Method::PATCH, "/dev/route4", "")
            .await,
        json!([4])
    );
}

#[test(modules = Deno)]
async fn method_object(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .prefix('/route1', {
                get: () => [1, 'get'],
                post: () => [1, 'post'],
            })
            .prefix('/route2', {
                'get|post': () => [2],
            })
            .prefix('/route3', {
                'DELETE': () => [3, "delete"],
                'patch': () => [3, "patch"],
            })
            .prefix('/route4', {
                '*': () => [4],
            });
        "#,
    );

    c.chisel.apply_ok().await;

    assert_eq!(
        c.chisel.request_json(Method::GET, "/dev/route1", "").await,
        json!([1, "get"])
    );
    assert_eq!(
        c.chisel.request_json(Method::POST, "/dev/route1", "").await,
        json!([1, "post"])
    );
    assert_eq!(
        c.chisel
            .request_status(Method::DELETE, "/dev/route1", "")
            .await,
        405
    );

    assert_eq!(
        c.chisel.request_json(Method::GET, "/dev/route2", "").await,
        json!([2])
    );
    assert_eq!(
        c.chisel.request_json(Method::POST, "/dev/route2", "").await,
        json!([2])
    );
    assert_eq!(
        c.chisel
            .request_status(Method::DELETE, "/dev/route2", "")
            .await,
        405
    );

    assert_eq!(
        c.chisel
            .request_json(Method::DELETE, "/dev/route3", "")
            .await,
        json!([3, "delete"])
    );
    assert_eq!(
        c.chisel
            .request_json(Method::PATCH, "/dev/route3", "")
            .await,
        json!([3, "patch"])
    );
    assert_eq!(
        c.chisel
            .request_status(Method::GET, "/dev/route3", "")
            .await,
        405
    );

    assert_eq!(
        c.chisel.request_json(Method::GET, "/dev/route4", "").await,
        json!([4])
    );
    assert_eq!(
        c.chisel.request_json(Method::POST, "/dev/route4", "").await,
        json!([4])
    );
    assert_eq!(
        c.chisel
            .request_json(Method::PATCH, "/dev/route4", "")
            .await,
        json!([4])
    );
}

#[test(modules = Deno)]
async fn errors(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/route1', function () { throw new Error("error") })
            // @ts-expect-error
            .get('/route2', () => nonexistent)
            .post('/route3', () => [3]);
        "#,
    );

    c.chisel.apply_ok().await;

    assert_eq!(
        c.chisel.get_body("/dev/route1").await,
        (500, Bytes::from(""))
    );
    assert_eq!(
        c.chisel.get_body("/dev/route2").await,
        (500, Bytes::from(""))
    );
    assert_eq!(
        c.chisel.get_body("/dev/route3").await,
        (405, Bytes::from(""))
    );
    assert_eq!(
        c.chisel.get_body("/dev/nonexistent").await,
        (404, Bytes::from(""))
    );
}
