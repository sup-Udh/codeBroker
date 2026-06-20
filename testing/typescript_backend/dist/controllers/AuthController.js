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
Object.defineProperty(exports, "__esModule", { value: true });
exports.AuthController = void 0;
const validator_1 = require("../utils/validator");
class AuthController {
    constructor(authService) {
        this.register = (req, res) => __awaiter(this, void 0, void 0, function* () {
            try {
                const { email, password, name } = req.body;
                const missing = (0, validator_1.validateRequiredFields)(req.body, ['email', 'password', 'name']);
                if (missing.length > 0) {
                    res.status(400).json({ error: `Missing required fields: ${missing.join(', ')}` });
                    return;
                }
                if (!(0, validator_1.isValidEmail)(email)) {
                    res.status(400).json({ error: 'Invalid email format' });
                    return;
                }
                if (!(0, validator_1.isStrongPassword)(password)) {
                    res.status(400).json({ error: 'Password must be at least 8 characters long' });
                    return;
                }
                const result = yield this.authService.register(email, password, name);
                res.status(201).json(result);
            }
            catch (error) {
                if (error.message === 'User already exists') {
                    res.status(409).json({ error: error.message });
                }
                else {
                    res.status(500).json({ error: 'Internal server error' });
                }
            }
        });
        this.login = (req, res) => __awaiter(this, void 0, void 0, function* () {
            try {
                const { email, password } = req.body;
                const missing = (0, validator_1.validateRequiredFields)(req.body, ['email', 'password']);
                if (missing.length > 0) {
                    res.status(400).json({ error: `Missing required fields: ${missing.join(', ')}` });
                    return;
                }
                const result = yield this.authService.login(email, password);
                res.status(200).json(result);
            }
            catch (error) {
                if (error.message === 'Invalid credentials') {
                    res.status(401).json({ error: error.message });
                }
                else {
                    res.status(500).json({ error: 'Internal server error' });
                }
            }
        });
        this.authService = authService;
    }
}
exports.AuthController = AuthController;
