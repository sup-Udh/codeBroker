const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');
const utils = require('../shared/utils');

class TokenManager {
  generateToken(user) {
    logger.info(`Generating token for ${user}`);
    EmailService.sendEmail('admin@crm.com', 'Token Generated', 'A new token was generated.');
    return utils.generateId() + utils.generateId();
  }
}
module.exports = new TokenManager();
