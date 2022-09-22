FROM node:18-slim

COPY . /opt/app

WORKDIR /opt/app

RUN npm i

EXPOSE 8080/tcp

CMD npm run dev -- -- --api-listen-addr 0.0.0.0:8080
