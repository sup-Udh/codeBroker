"use strict";
var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
var __rest = (this && this.__rest) || function (s, e) {
    var t = {};
    for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p) && e.indexOf(p) < 0)
        t[p] = s[p];
    if (s != null && typeof Object.getOwnPropertySymbols === "function")
        for (var i = 0, p = Object.getOwnPropertySymbols(s); i < p.length; i++) {
            if (e.indexOf(p[i]) < 0 && Object.prototype.propertyIsEnumerable.call(s, p[i]))
                t[p[i]] = s[p[i]];
        }
    return t;
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.UserController = void 0;
class UserController {
    constructor(userService) {
        this.getAllUsers = (req, res) => __awaiter(this, void 0, void 0, function* () {
            try {
                const users = yield this.userService.getAllUsers();
                // Filter out password hashes
                const safeUsers = users.map((_a) => {
                    var { passwordHash } = _a, user = __rest(_a, ["passwordHash"]);
                    return user;
                });
                res.status(200).json(safeUsers);
            }
            catch (error) {
                res.status(500).json({ error: 'Internal server error' });
            }
        });
        this.getUserById = (req, res) => __awaiter(this, void 0, void 0, function* () {
            var _a, _b;
            try {
                const id = req.params.id;
                // Allow users to get their own profile or admin to get any
                if (((_a = req.user) === null || _a === void 0 ? void 0 : _a.role) !== 'admin' && ((_b = req.user) === null || _b === void 0 ? void 0 : _b.userId) !== id) {
                    res.status(403).json({ error: 'Forbidden' });
                    return;
                }
                const user = yield this.userService.getUserById(id);
                const { passwordHash } = user, safeUser = __rest(user, ["passwordHash"]);
                res.status(200).json(safeUser);
            }
            catch (error) {
                if (error.message === 'User not found') {
                    res.status(404).json({ error: error.message });
                }
                else {
                    res.status(500).json({ error: 'Internal server error' });
                }
            }
        });
        this.getProfile = (req, res) => __awaiter(this, void 0, void 0, function* () {
            try {
                const id = req.user.userId;
                const user = yield this.userService.getUserById(id);
                const { passwordHash } = user, safeUser = __rest(user, ["passwordHash"]);
                res.status(200).json(safeUser);
            }
            catch (error) {
                res.status(500).json({ error: 'Internal server error' });
            }
        });
        this.userService = userService;
    }
}
exports.UserController = UserController;
