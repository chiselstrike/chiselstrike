// Example ChiselStrike route
//
// To access the route, run:
//
// curl -d '{"hello": "world"}' localhost:8080/dev/hello

import { RouteMap, ChiselRequest } from '@chiselstrike/api';

export default new RouteMap()
    .get('/', function (req: ChiselRequest): string {
        return 'hello world';
    })
    .post('/', async function (req: ChiselRequest): Promise<unknown> {
        return await req.json();
    });


