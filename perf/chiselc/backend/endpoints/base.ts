import { User } from "../models/user";

export default async function (req: Request): Promise<Response> {
    for await (const user of User.cursor()) {
        if ((user.name === "Glauber Costa") && (user.age >= 40)) {
            return new Response(user.email);
        }
    }
    return new Response("not found");
}
