use crate::framework::prelude::*;
use serde_json::json;

pub async fn basic(c: TestContext) {
    c.chisel.write("routes/index.ts", r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/person', () => "GET index.ts /person");
        "#);

    c.chisel.write("routes/user.ts", r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/', () => "GET user.ts /")
            .get('/alice', () => "GET user.ts /alice");
        "#);

    c.chisel.write("routes/user/index.ts", r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/bob', () => "GET user/index.ts /bob");
        "#);

    c.chisel.write("routes/user/helen.ts", r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/', () => "GET user/helen.ts /");
        "#);

    c.chisel.apply_ok().await;

    assert_eq!(c.chisel.get_text("/dev/person").await, "GET index.ts /person");
    assert_eq!(c.chisel.get_text("/dev/user").await, "GET user.ts /");
    assert_eq!(c.chisel.get_text("/dev/user/alice").await, "GET user.ts /alice");
    assert_eq!(c.chisel.get_text("/dev/user/bob").await, "GET user/index.ts /bob");
    assert_eq!(c.chisel.get_text("/dev/user/helen").await, "GET user/helen.ts /");
}

pub async fn params_in_code(c: TestContext) {
    c.chisel.write("routes/index.ts", r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('/route1/:x',
                (req) => [1, req.params.get('x')])
            .get('/route2/:x/:y',
                (req) => [2, req.params.get('x'), req.params.get('y')])
            .get('/:x/route3/:y',
                (req) => [3, req.params.get('x'), req.params.get('y')]);
        "#);

    c.chisel.apply_ok().await;

    assert_eq!(c.chisel.get_json("/dev/route1/xyz").await, json!([1, "xyz"]));
    assert_eq!(c.chisel.get_json("/dev/route1/!@$^").await, json!([1, "!@$^"]));
    assert_eq!(c.chisel.get_status("/dev/route1/").await, 404);
    assert_eq!(c.chisel.get_status("/dev/route1/xyz/abc").await, 404);

    assert_eq!(c.chisel.get_json("/dev/route2/abc/xyz").await, json!([2, "abc", "xyz"]));
    assert_eq!(c.chisel.get_json("/dev/abc/route3/xyz").await, json!([3, "abc", "xyz"]));
}

pub async fn params_in_files(c: TestContext) {
    c.chisel.write("routes/route1/[x].ts", r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('', (req) => [1, req.params.get('x')]);
        "#);

    c.chisel.write("routes/route2/[x]/[y].ts", r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('', (req) => [2, req.params.get('x'), req.params.get('y')]);
        "#);

    c.chisel.write("routes/[x]/route3/[y].ts", r#"
        import { RouteMap } from '@chiselstrike/api';
        export default new RouteMap()
            .get('', (req) => [3, req.params.get('x'), req.params.get('y')]);
        "#);

    assert_eq!(c.chisel.get_json("/dev/route1/xyz").await, json!([1, "xyz"]));
    assert_eq!(c.chisel.get_json("/dev/route2/abc/xyz").await, json!([2, "abc", "xyz"]));
    assert_eq!(c.chisel.get_json("/dev/abc/route3/xyz").await, json!([3, "abc", "xyz"]));
}
