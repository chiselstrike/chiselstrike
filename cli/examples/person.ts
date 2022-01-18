/* SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com> */

export class Person extends Chisel.ChiselEntity {
  first_name: string = "";
  @labels("pii") last_name: string = "";
  age: number = 0;
  human: boolean = false;
  height: number = 1;
}


export class Position extends Chisel.ChiselEntity {
  title: string = "title";
}
