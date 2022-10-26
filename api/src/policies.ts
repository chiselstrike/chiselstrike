export type ReqContext = {
    path: string;
    method: string;
    headers: Record<string, string>;
    apiVersion: string;
    userId: string;
};

export const Action = {
    Allow: 0,
    Log: 1,
    Deny: 2,
    Skip: 3,
};

export class PermissionDeniedError extends Error {
    constructor(msg: string) {
        super(msg);
    }
}

export class DirtyEntityError extends Error {
    constructor(msg: string) {
        super(msg);
    }
}
