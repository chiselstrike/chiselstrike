import { faker } from '@faker-js/faker';

import { User } from "../models/user.ts";

export default async function (req: Request): Response {
    for (let i = 0; i < 100000; i++) {
        const name = faker.name.findName();
        const email = faker.internet.email();
        await User.create({ name, email });
    }
    return new Response("ok");
}
