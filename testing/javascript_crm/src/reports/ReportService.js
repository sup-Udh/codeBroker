const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');
const ReportGenerator = require('./ReportGenerator');

class ReportService {
  sendWeeklyReport() {
    logger.info('Sending weekly report');
    const reportData = ReportGenerator.generateWeekly();
    EmailService.sendEmail('execs@crm.com', 'Weekly Report', reportData);
  }
}
module.exports = new ReportService();
