import { faker } from '@faker-js/faker';

import { User } from "../models/user";

export default async function (req: Request): Promise<Response> {
    for (let i = 0; i < 1000; i++) {
        const promises = [];
        for (let j = 0; j < 1000; j++) {
            const name = faker.name.findName();
            const email = faker.internet.email();
            const age = parseInt(faker.random.numeric(2));
            promises.push(User.create({ name, email, age }));
        }
        await Promise.all(promises);
        console.log(`done ${i}`);
    }
    await User.create({ name: "Glauber Costa", email: "glauber@nospam.me", age: 40 });
    await User.create({ name: "Glauber Costa", email: "glauberjr@nospam.me", age: 15 });
    return new Response("ok");
}
