import { Database } from "./storage";

class JSTestService {
    constructor(db) {
        this.db = db;
    }

    doSomething() {
        console.log("Doing something in JS");
    }
}
