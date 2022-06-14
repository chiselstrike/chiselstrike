import { User } from "../models/user";
import { validate } from "email-validator";

export default async function (req: Request): Promise<Response> {
    const user = await User.findOne((user) =>
        user.name == "Glauber Costa" && user.age >= 40 && validate(user.email)
    );
    return new Response(user?.email ?? "not found");
}
