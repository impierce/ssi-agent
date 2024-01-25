import './app.css'
import { Body } from './components/body';
import { Navigation } from './components/navigation-menu';

export function App() {
    console.log("config", import.meta.env.VITE_APPLICATION_URL);

    return (
        <div>
            <Navigation />
            <Body>
                <h1>Unicore portal</h1>
                <div><span>Url: </span><span>{import.meta.env.VITE_APPLICATION_URL}</span></div>

                <div>

                </div>

                <div>
                    Show known-authorization
                </div>
            </Body>
        </div>
    )
}
