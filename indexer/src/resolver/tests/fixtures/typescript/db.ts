export interface IDatabase {
    query(sql: string): any;
}

export class Database implements IDatabase {
    query(sql: string) { return []; }
}

export function createDb(): Database {
    return new Database();
}
