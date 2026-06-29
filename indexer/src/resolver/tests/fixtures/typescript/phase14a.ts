import { UserService } from "./service";
import { Database as DbAlias } from "./db";

// 1. Imports
const service = new UserService();
service.login(); // Resolves to UserService.login

// 2. Alias
const a = b;
const b = c;
const c = db;
db.query(); // db resolves to Database, c, b, a do too
a.query(); // Resolves to Database.query

// 3. Nested Property
const client = new Client();
client.database.query(); // Resolves to Database.query

// 4. Constructor
const database = new Database();
database.query(); // Resolves to Database.query

// 5. Return Flow
function createDatabase(): Database {
    return new Database();
}
const myDb = createDatabase();
myDb.query(); // Resolves to Database.query

// 6. Destructuring
const ctx = { db: new Database() };
const { db: destrDb } = ctx;
destrDb.query(); // Resolves to Database.query

// 7. Object Literal
const api = {
    db: new Database()
};
api.db.query(); // Resolves to Database.query

// 8. Cyclic Alias
const cycleA = cycleB;
const cycleB = cycleA;
cycleA.query(); // Should terminate safely and resolve to Unknown/Dynamic
