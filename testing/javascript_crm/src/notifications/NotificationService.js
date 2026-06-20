const logger = require('../shared/logger');
const EmailService = require('./EmailService');
const SmsService = require('./SmsService');

class NotificationService {
  notifyUser(user, message) {
    logger.info(`Notifying user ${user.name}`);
    EmailService.sendEmail(user.email, 'Notification', message);
    if (user.phone) {
      SmsService.sendSms(user.phone, message);
    }
  }
}
module.exports = new NotificationService();
