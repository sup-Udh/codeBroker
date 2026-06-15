import { Database } from "./storage";

export class TestService {
    constructor(private db: Database) {}

    public doSomething() {
        console.log("Doing something");
    }
}

export interface ITestService {
    doSomething(): void;
}
