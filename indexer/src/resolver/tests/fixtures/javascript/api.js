import { Database, createDb } from './db.js';

export class ApiClient {
    constructor() {
        this.db = createDb();
    }
    
    fetchUser() {
        return this.db.query("SELECT * FROM users");
    }
}
