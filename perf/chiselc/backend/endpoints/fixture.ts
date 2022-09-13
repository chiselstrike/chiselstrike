// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { faker } from "@faker-js/faker";

import { User } from "../models/user";

function randomInt(min: number, max: number) {
    min = Math.ceil(min);
    max = Math.floor(max);
    return Math.floor(Math.random() * (max - min + 1)) + min;
}

// call fixture/<number> to generate <number> entries
export default async function (req: ChiselRequest): Promise<Response> {
    const rr = req.pathParams;
    const parsed = parseInt(req.pathParams);
    const populationSize = isNaN(parsed) ? 0 : parsed;

    const glauberPos = randomInt(0, populationSize);
    const glauberJrPos = randomInt(0, populationSize);

    const promises = [];
    for (let i = 0; i < populationSize; i++) {
        promises.push(User.create({
            name: faker.name.findName(),
            email: faker.internet.email(),
            age: randomInt(0, 100),
        }));

        if (i % 1000 == 0) {
            await Promise.all(promises);
            promises.length = 0;
        }

        if (i == glauberPos) {
            promises.push(User.create({
                name: "Glauber Costa",
                email: "glauber@nospam.me",
                age: 40,
            }));
        }
        if (i == glauberJrPos) {
            promises.push(User.create({
                name: "Glauber Costa",
                email: "glauberjr@nospam.me",
                age: 15,
            }));
        }
    }
    await Promise.all(promises);
    return new Response(`inserted ${populationSize}`);
}
