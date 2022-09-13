// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { User } from "../models/user";

export default async function (req: Request): Promise<Response> {
    const user = await User.findOne((user) =>
        user.name == "Glauber Costa" && user.age >= 40
    );
    return new Response(user?.email ?? "not found");
}
