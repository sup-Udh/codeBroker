export class Database {
    query(sql) { return []; }
}

export function createDb() {
    return new Database();
}
