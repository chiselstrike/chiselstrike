import Head from 'next/head'
import Image from 'next/image'
import Link from 'next/link'
import styles from '../styles/Home.module.css'
import React, {useEffect} from "react";
import { chiselFetch, getChiselStrikeClient } from "../lib/chiselstrike";
import { withSessionSsr } from "../lib/withSession";

export const getServerSideProps = withSessionSsr(
    async function getServerSideProps(context) {
        const chisel = await getChiselStrikeClient(context.req.session, context.query);
        return { props: { chisel } };
    },
);

export default function Home({ chisel }) {
  const [peopleData, setPeopleData] = React.useState([])

  async function fetch_people() {
    const res = await chiselFetch(chisel, 'api/get_all_people', {
      method: 'GET',
    });
    const jsonData = await res.json();
    setPeopleData(jsonData)
  }
  useEffect(fetch_people, [])
  const defaultState = {
    firstName: "",
    lastName: ""
  }

  const [state, setState] = React.useState(defaultState)
  function handleChange(evt) {
    const value = evt.target.value;
    setState({
      ...state,
      [evt.target.name]: value
    });
  }

  const submitPerson = async (event) => {
    event.preventDefault() // don't redirect the page
    await chiselFetch(chisel, 'api/import_person', {
      method: 'PUT',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(state),
    });
    await fetch_people();
    setState(defaultState)
  }

  const greeting = chisel.user ?
        <p>Hello, {chisel.user}. Click <Link href='/api/logout'>here</Link> to log out.</p> :
        <p>Hello, anonymous. Click <Link href={chisel.loginLink}>here</Link> to log in.</p>;

  return (
    <div>
      { greeting }
      <form onSubmit={submitPerson}>
        <label>
          First name: 
          <input
            type="text"
            name="firstName"
            value={state.firstName}
            onChange={handleChange}
          />
        </label>
        <label>
          Last name: 
          <input
            type="text"
            name="lastName"
            value={state.lastName}
            onChange={handleChange}
          />
        </label>
        <button type="submit">Submit Person</button>
      </form>
      <table>
        <tbody>
          <tr>
            <td>firstName</td>
            <td>lastName</td>
          </tr>
          {peopleData.map((person) => (
            <tr>
              <td>{person.firstName}</td>
              <td>{person.lastName}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
