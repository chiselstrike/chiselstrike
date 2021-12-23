// next.config.js
module.exports = {
    async rewrites() {
        return [
          {
            source: '/api/:path*',
            destination: 'http://127.0.0.1:8080/dev/api/:path*',
          },
        ]
      },
  };
