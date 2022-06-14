import { User } from "../models/user";

export default async function (req: Request): Promise<Response> {
    await User.create({
        name: "Glauber Costa",
        email: "glauber@nospam.me",
        age: 40,
    });
    await User.create({
        name: "Glauber Costa",
        email: "glauberjr@nospam.me",
        age: 15,
    });
    return new Response("ok");
}
