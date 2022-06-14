import { ChiselEntity } from "@chiselstrike/api";

export class User extends ChiselEntity {
    name: string;
    email: string;
    age: number;
}
