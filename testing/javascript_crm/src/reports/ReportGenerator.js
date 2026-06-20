const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');

class ReportGenerator {
  generateWeekly() {
    logger.info('Generating weekly report data');
    EmailService.sendEmail('admin@crm.com', 'Report Generation Started', 'Generating weekly report.');
    return 'Report: 100 leads, 20 opportunities';
  }
}
module.exports = new ReportGenerator();
