// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { User } from "../models/user";
import { validate } from "email-validator";

export default async function (req: Request): Promise<Response> {
    const user = await User.findOne((user) =>
        user.name == "Glauber Costa" && user.age >= 40
    );
    return new Response(validate(user?.email) ? user.email : "not found");
}
