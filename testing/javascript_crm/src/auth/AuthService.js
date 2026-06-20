const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');
const TokenManager = require('./TokenManager');

class AuthService {
  login(username, password) {
    logger.info(`Login attempt for ${username}`);
    EmailService.sendEmail('security@crm.com', 'Login Event', `User ${username} logged in.`);
    return TokenManager.generateToken(username);
  }
}
module.exports = new AuthService();
