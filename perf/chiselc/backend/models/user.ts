// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { ChiselEntity } from "@chiselstrike/api";

export class User extends ChiselEntity {
    name: string;
    email: string;
    age: number;
}
