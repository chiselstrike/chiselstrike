import * as tsm from "ts-morph";

import { assert, assertEquals } from "./utils";

export type ReflectionType =
    | { name: "undefined" }
    | { name: "string" }
    | { name: "number" }
    | { name: "boolean" }
    | { name: "date" }
    | { name: "arrayBuffer" }
    | { name: "array"; elementType: ReflectionType }
    | { name: "namedObject"; typeName: string; fields: Record<string, ReflectionType> }
    | { name: "anonymousObject"; fields: Record<string, ReflectionType> };

export function getTypeReflection(tc: tsm.TypeChecker, type: tsm.Type): ReflectionType {
    if (type.isUndefined()) {
        return { name: "undefined" };
    } else if (type.isString()) {
        return { name: "string" };
    } else if (type.isNumber()) {
        return { name: "number" };
    } else if (type.isBoolean()) {
        return { name: "boolean" };
    } else if (type.isArray()) {
        const elementType = type.getArrayElementTypeOrThrow();
        return { name: "array", elementType: getTypeReflection(tc, elementType) };
    } else if (type.isObject()) {
        return getObjectReflection(tc, type);
    } else {
        throw new Error("TODO");
    }
}

function getObjectReflection(tc: tsm.TypeChecker, type: tsm.Type): ReflectionType {
    assert(type.isObject());
    const symbol = type.getSymbol();
    if (
        symbol !== undefined &&
        symbol.getName() === "Date" &&
        type.getProperty("getTime") !== undefined
    ) {
        return { name: "date" };
    } else {
        const fields: Record<string, ReflectionType> = {};
        for (const property of type.getProperties()) {
            const name = property.getName();
            const propertyType = tc.getTypeAtLocation(property.getValueDeclarationOrThrow());
            fields[name] = getTypeReflection(tc, propertyType);
        }
        if (type.isClassOrInterface()) {
            const typeName = type.getSymbolOrThrow().getName();
            return { name: "namedObject", typeName, fields };
        } else if (type.isAnonymous()) {
            return { name: "anonymousObject", fields };
        } else {
            throw new Error("Unexpected object type");
        }
    }
}
