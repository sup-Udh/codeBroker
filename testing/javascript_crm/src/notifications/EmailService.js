// Everything depends on this
const logger = require('../shared/logger');
const config = require('../shared/config');

class EmailService {
  sendEmail(to, subject, body) {
    logger.info(`Sending email to ${to} via ${config.email.provider}... Subject: ${subject}`);
    // Simulate email sending
    return true;
  }
}
module.exports = new EmailService();
