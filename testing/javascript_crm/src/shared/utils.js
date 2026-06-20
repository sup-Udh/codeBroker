const logger = require('./logger');

module.exports = {
  generateId: () => {
    logger.info('Generating new ID');
    return Math.random().toString(36).substr(2, 9);
  },
  formatDate: (date) => {
    logger.info('Formatting date');
    return new Date(date).toISOString();
  }
};
