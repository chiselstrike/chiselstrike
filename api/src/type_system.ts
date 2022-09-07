// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

export type SimpleTypeSystem = {
    customEntities: Record<string, Entity>;
    builtinEntities: Record<string, Entity>;
};

export type PrimitiveType =
    | { name: "string" }
    | { name: "number" }
    | { name: "boolean" }
    | { name: "jsDate" }
    | { name: "array"; elementType: PrimitiveType };

export type EntityId = {
    name: "entity";
    entityName: string;
};

export type Field = {
    name: string;
    type: PrimitiveType | EntityId;
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
        if (name in this.ts.customEntities) {
            return this.ts.customEntities[name];
        } else if (name in this.ts.builtinEntities) {
            return this.ts.builtinEntities[name];
        }
        return undefined;
    }
}
