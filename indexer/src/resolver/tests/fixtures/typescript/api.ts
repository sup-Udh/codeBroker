import { Database, createDb } from './db';

export class ApiClient {
    public db: Database;
    
    constructor() {
        this.db = createDb();
    }
    
    public fetchUser() {
        return this.db.query("SELECT * FROM users");
    }
}
