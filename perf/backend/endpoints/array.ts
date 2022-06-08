import { User } from "../models/user";

export default async function (req: Request): Promise<Response> {
    const all = await User.findAll();
    const user = all.find(user => user.name == "Glauber Costa" && user.age >= 40);
    return new Response(user?.email ?? "not found");
}
