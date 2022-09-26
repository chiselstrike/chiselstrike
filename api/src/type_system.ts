// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

export type SimpleTypeSystem = {
    entities: Record<string, Entity>;
};

export type Type =
    | { name: "string" }
    | { name: "number" }
    | { name: "boolean" }
    | { name: "jsDate" }
    | { name: "array"; elementType: Type }
    | { name: "entity"; entityName: string }
    | { name: "entityId"; entityName: string };

export type Field = {
    name: string;
    type: Type;
    isOptional: boolean;
    isUnique: boolean;
};

export type Entity = {
    name: string;
    fields: Field[];
};

export class TypeSystem {
    constructor(public ts: SimpleTypeSystem) {}

    public findEntity(name: string): Entity | undefined {
        return this.ts.entities[name];
    }
}
