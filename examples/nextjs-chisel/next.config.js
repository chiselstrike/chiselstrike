// next.config.js
module.exports = {
    async rewrites() {
        return [
          {
            source: '/api/:path*',
            destination: 'http://localhost:8080/api/:path*',
          },
        ]
      },
  };
