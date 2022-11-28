export type Type =
    | { name: "string" }
    | { name: "number" }
    | { name: "boolean" }
    | { name: "date" }
    | { name: "arrayBuffer" }
    | { name: "array"; elementType: Type }
    | { name: "entity"; entityType: Entity }
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
