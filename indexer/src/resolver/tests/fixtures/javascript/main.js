import { ApiClient } from './api.js';

// Destructuring
const { db } = new ApiClient();
db.query("...");

// Object literals & nested chains
const config = {
    api: new ApiClient()
};
config.api.db.query("...");

// Aliases
const a = config;
const b = a;
b.api.fetchUser();

// Factory functions
function wrap(val) { return val; }
const wrapped = wrap(new ApiClient());
wrapped.fetchUser();

// Reexports
export { ApiClient as Client } from './api.js';

// Recursive aliases
let x = new ApiClient();
let y = x;
x = y;
y.fetchUser();
