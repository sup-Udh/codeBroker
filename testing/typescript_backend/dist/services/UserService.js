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
exports.UserService = void 0;
class UserService {
    constructor(userRepository) {
        this.userRepository = userRepository;
    }
    getAllUsers() {
        return __awaiter(this, void 0, void 0, function* () {
            return this.userRepository.findAll();
        });
    }
    getUserById(id) {
        return __awaiter(this, void 0, void 0, function* () {
            const user = yield this.userRepository.findById(id);
            if (!user) {
                throw new Error('User not found');
            }
            return user;
        });
    }
    updateUser(id, data) {
        return __awaiter(this, void 0, void 0, function* () {
            // Ensure user exists
            yield this.getUserById(id);
            const updated = yield this.userRepository.update(id, data);
            if (!updated) {
                throw new Error('Failed to update user');
            }
            return updated;
        });
    }
    deleteUser(id) {
        return __awaiter(this, void 0, void 0, function* () {
            const deleted = yield this.userRepository.delete(id);
            if (!deleted) {
                throw new Error('User not found or could not be deleted');
            }
        });
    }
}
exports.UserService = UserService;
