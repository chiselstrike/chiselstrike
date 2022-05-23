import { User } from "../models/user.ts";

export default async function (req: Request): Response {
    const users = await User.cursor().filter(user => user.name == "Alvin Wisoky").toArray();
    const user = users[0];
    return new Response("Found user: " + user.id);
}
