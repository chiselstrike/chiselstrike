/* SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com> */

class Person extends ChiselEntity {
  first_name: string;
  @labels("pii") last_name: string;
  age: number;
  human: boolean;
  height: number;
}


class Position extends ChiselEntity {
  title: string;
}
