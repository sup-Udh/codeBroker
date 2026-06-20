"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.validateRequiredFields = exports.isStrongPassword = exports.isValidEmail = void 0;
const isValidEmail = (email) => {
    const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
    return emailRegex.test(email);
};
exports.isValidEmail = isValidEmail;
const isStrongPassword = (password) => {
    return password.length >= 8;
};
exports.isStrongPassword = isStrongPassword;
const validateRequiredFields = (obj, fields) => {
    const missing = [];
    for (const field of fields) {
        if (obj[field] === undefined || obj[field] === null || obj[field] === '') {
            missing.push(field);
        }
    }
    return missing;
};
exports.validateRequiredFields = validateRequiredFields;
