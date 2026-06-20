const logger = require('../shared/logger');
const EmailService = require('./EmailService');

class SmsService {
  sendSms(phone, message) {
    logger.info(`Sending SMS to ${phone}`);
    // Fallback to email if SMS fails (simulated)
    EmailService.sendEmail('admin@crm.com', 'SMS Failed', `Failed to send SMS to ${phone}`);
  }
}
module.exports = new SmsService();
